use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::backend::ffmpeg::capability::probe_capabilities;
use crate::backend::ffmpeg::command_builder::{build_ffmpeg_args, build_ffprobe_args};
use crate::backend::ffmpeg::locator::FfmpegLocations;
use crate::backend::ffmpeg::progress_parser::ProgressParser;
use crate::backend::{BackendCapabilities, MediaBackend, ProgressSink};
use crate::domain::conversion_job::ExecutionReport;
use crate::domain::media_info::MediaInfo;
use crate::error::{RuviError, RuviResult};
use crate::planner::ConversionPlan;
use crate::probe::parse_ffprobe_json;

const MAX_STDERR_LINES: usize = 64;

pub struct FfmpegBackend {
    locations: FfmpegLocations,
    timeout: Duration,
}

impl FfmpegBackend {
    pub fn new(locations: FfmpegLocations, timeout: Duration) -> Self {
        Self { locations, timeout }
    }
}

#[async_trait]
impl MediaBackend for FfmpegBackend {
    async fn capabilities(&self) -> RuviResult<BackendCapabilities> {
        probe_capabilities(&self.locations.ffmpeg, &self.locations.ffprobe).await
    }

    async fn probe(&self, input: &Path) -> RuviResult<MediaInfo> {
        let input_str = input.to_string_lossy().into_owned();
        let args = build_ffprobe_args(&input_str);
        let mut cmd = Command::new(&self.locations.ffprobe);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = tokio::time::timeout(Duration::from_secs(30), cmd.output())
            .await
            .map_err(|_| RuviError::Backend("ffprobe timed out".to_string()))?
            .map_err(|err| {
                RuviError::Backend(format!(
                    "failed to launch ffprobe ({}): {err}",
                    self.locations.ffprobe.display()
                ))
            })?;

        if !output.status.success() {
            let stderr = sanitize_log_line(&String::from_utf8_lossy(&output.stderr));
            return Err(RuviError::Backend(format!(
                "ffprobe failed ({}): {}",
                output.status,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_ffprobe_json(&stdout, &input_str)
    }

    async fn execute(
        &self,
        plan: &ConversionPlan,
        progress: &dyn ProgressSink,
        cancel: CancellationToken,
    ) -> RuviResult<ExecutionReport> {
        let start = Instant::now();
        let args = build_ffmpeg_args(plan);
        tracing::debug!(bin = %self.locations.ffmpeg.display(), arg_count = args.len(), "launching ffmpeg");

        let mut cmd = Command::new(&self.locations.ffmpeg);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|err| {
            RuviError::Backend(format!(
                "failed to spawn ffmpeg ({}): {err}",
                self.locations.ffmpeg.display()
            ))
        })?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (prog_tx, mut prog_rx) = tokio::sync::mpsc::unbounded_channel();

        let stdout_task = tokio::spawn(async move {
            let Some(out) = stdout else {
                return;
            };
            let mut reader = BufReader::new(out).lines();
            let mut parser = ProgressParser::new();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Some(update) = parser.push_line(&line) {
                    let _ = prog_tx.send(update);
                }
            }
        });

        let stderr_task = tokio::spawn(async move {
            let Some(err_stream) = stderr else {
                return Vec::new();
            };
            let mut reader = BufReader::new(err_stream).lines();
            let mut tail: VecDeque<String> = VecDeque::with_capacity(MAX_STDERR_LINES);
            while let Ok(Some(line)) = reader.next_line().await {
                if tail.len() >= MAX_STDERR_LINES {
                    tail.pop_front();
                }
                tail.push_back(sanitize_log_line(&line));
            }
            tail.into_iter().collect::<Vec<_>>()
        });

        let sleep_deadline = tokio::time::sleep(self.timeout);
        tokio::pin!(sleep_deadline);
        let mut prog_closed = false;

        // Drive child wait + live progress updates under timeout + cancellation + ctrl-c
        let status = loop {
            tokio::select! {
                maybe_update = prog_rx.recv(), if !prog_closed => {
                    match maybe_update {
                        Some(update) => progress.on_progress(update),
                        None => prog_closed = true,
                    }
                }
                res = child.wait() => {
                    break res.map_err(|err| RuviError::Backend(format!("failed waiting for ffmpeg: {err}")))?;
                }
                () = &mut sleep_deadline => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(RuviError::Backend(format!(
                        "ffmpeg exceeded timeout of {}s",
                        self.timeout.as_secs()
                    )));
                }
                () = cancel.cancelled() => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(RuviError::Cancelled("ffmpeg operation cancelled".to_string()));
                }
                ctrl = tokio::signal::ctrl_c() => {
                    if ctrl.is_ok() {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        return Err(RuviError::Cancelled("interrupted by Ctrl-C".to_string()));
                    }
                }
            }
        };

        let _ = stdout_task.await;
        while let Ok(update) = prog_rx.try_recv() {
            progress.on_progress(update);
        }
        let stderr_tail = stderr_task.await.unwrap_or_default();

        let code = status.code().unwrap_or(-1);
        if !status.success() {
            let msg = if stderr_tail.is_empty() {
                format!("ffmpeg exited with status {code}")
            } else {
                format!(
                    "ffmpeg exited with status {code}: {}",
                    stderr_tail.join(" | ")
                )
            };
            return Err(RuviError::Backend(msg));
        }

        Ok(ExecutionReport {
            exit_code: code,
            stderr_tail,
            selected_video_encoder: plan.video_encoder.clone(),
            selected_audio_encoder: plan.audio_encoder.clone(),
            hwaccel_requested: plan.hwaccel_requested.as_str().to_string(),
            hwaccel_used: plan.hwaccel_input_flag.clone(),
            hwaccel_fallback_reason: plan.hwaccel_fallback_reason.clone(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

pub fn sanitize_log_line(line: &str) -> String {
    let mut out = line.to_string();
    for secret_key in ["token=", "sig=", "key=", "auth=", "password="] {
        if let Some(idx) = out.to_ascii_lowercase().find(secret_key) {
            let prefix = &out[..idx + secret_key.len()];
            let rest = &out[idx + secret_key.len()..];
            let end = rest.find(['&', ' ', '"', '\'']).unwrap_or(rest.len());
            out = format!("{prefix}[REDACTED]{}", &rest[end..]);
        }
    }
    out
}
