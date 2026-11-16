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
    cached_caps: tokio::sync::OnceCell<BackendCapabilities>,
}

impl FfmpegBackend {
    pub fn new(locations: FfmpegLocations, timeout: Duration) -> Self {
        Self {
            locations,
            timeout,
            cached_caps: tokio::sync::OnceCell::new(),
        }
    }
}

#[async_trait]
impl MediaBackend for FfmpegBackend {
    async fn capabilities(&self) -> RuviResult<BackendCapabilities> {
        self.cached_caps
            .get_or_try_init(|| probe_capabilities(&self.locations.ffmpeg, &self.locations.ffprobe))
            .await
            .cloned()
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
            let stderr = String::from_utf8_lossy(&output.stderr);
            let sanitized_tail: Vec<String> = stderr
                .lines()
                .rev()
                .take(MAX_STDERR_LINES)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(sanitize_log_line)
                .collect();
            return Err(RuviError::Backend(format!(
                "ffprobe failed ({}): {}",
                output.status,
                sanitized_tail.join(" | ").trim()
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
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut ctrl_c_armed = true;
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
                    stdout_task.abort();
                    stderr_task.abort();
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return Err(RuviError::Backend(format!(
                        "ffmpeg exceeded timeout of {}s",
                        self.timeout.as_secs()
                    )));
                }
                () = cancel.cancelled() => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    stdout_task.abort();
                    stderr_task.abort();
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return Err(RuviError::Cancelled("ffmpeg operation cancelled".to_string()));
                }
                ctrl = &mut ctrl_c, if ctrl_c_armed => {
                    ctrl_c_armed = false;
                    if ctrl.is_ok() {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        stdout_task.abort();
                        stderr_task.abort();
                        let _ = stdout_task.await;
                        let _ = stderr_task.await;
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

/// Redacts URL userinfo, auth headers, and sensitive query parameters from backend logs.
pub fn sanitize_log_line(line: &str) -> String {
    let step1 = redact_url_userinfo(line);
    let step2 = redact_auth_schemes(&step1);
    redact_key_value_secrets(&step2)
}

fn redact_url_userinfo(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = input[pos..].find("://") {
        let scheme_end = pos + rel + 3;
        let auth_end_rel = input[scheme_end..]
            .find([
                '/', '?', '#', ' ', '\t', '\r', '\n', '"', '\'', ')', ']', '}', ',', ';', '<', '>',
            ])
            .unwrap_or(input.len() - scheme_end);
        let authority = &input[scheme_end..scheme_end + auth_end_rel];
        if let Some(at_rel) = authority.rfind('@') {
            out.push_str(&input[pos..scheme_end]);
            out.push_str("[REDACTED]@");
            pos = scheme_end + at_rel + 1;
        } else {
            out.push_str(&input[pos..scheme_end]);
            pos = scheme_end;
        }
    }
    out.push_str(&input[pos..]);
    out
}

fn redact_auth_schemes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut last_copied = 0;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        let matched = ["bearer ", "basic "].into_iter().find(|scheme| {
            input[i..]
                .get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        });
        if let Some(scheme) = matched
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        {
            let mut val_start = i + scheme.len();
            while val_start < bytes.len() && matches!(bytes[val_start], b' ' | b'\t') {
                val_start += 1;
            }
            let val_end_rel = input[val_start..]
                .find([
                    ' ', '\t', '\r', '\n', '"', '\'', '&', ')', ']', '}', ',', ';', '<', '>',
                ])
                .unwrap_or(input.len() - val_start);
            if val_end_rel > 0 {
                out.push_str(&input[last_copied..val_start]);
                out.push_str("[REDACTED]");
                i = val_start + val_end_rel;
                last_copied = i;
                continue;
            }
        }
        let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
        i += ch_len;
    }

    out.push_str(&input[last_copied..]);
    out
}

fn redact_key_value_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_copied = 0;
    let bytes = input.as_bytes();

    for (eq_idx, _) in input.match_indices('=') {
        if eq_idx < last_copied {
            continue;
        }
        let mut key_start = eq_idx;
        while key_start > last_copied {
            let b = bytes[key_start - 1];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
                key_start -= 1;
            } else {
                break;
            }
        }
        let key = &input[key_start..eq_idx];
        if !is_sensitive_param_key(key) {
            continue;
        }
        let val_start = eq_idx + 1;
        let val_end_rel = input[val_start..]
            .find([
                '&', '#', ' ', '\t', '\r', '\n', '"', '\'', ')', ']', '}', ',', ';', '<', '>',
            ])
            .unwrap_or(input.len() - val_start);
        out.push_str(&input[last_copied..val_start]);
        out.push_str("[REDACTED]");
        last_copied = val_start + val_end_rel;
    }

    out.push_str(&input[last_copied..]);
    out
}

fn is_sensitive_param_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k == "token"
        || k.ends_with("_token")
        || k.ends_with("-token")
        || k == "authtoken"
        || k == "sig"
        || k == "signature"
        || k.ends_with("_sig")
        || k.ends_with("-sig")
        || k.ends_with("_signature")
        || k.ends_with("-signature")
        || k == "key"
        || k == "apikey"
        || k.ends_with("_key")
        || k.ends_with("-key")
        || k == "auth"
        || k == "authorization"
        || k.ends_with("_auth")
        || k.ends_with("-auth")
        || k == "password"
        || k == "passwd"
        || k == "pwd"
        || k.ends_with("_password")
        || k.ends_with("-password")
        || k.ends_with("_passwd")
        || k.ends_with("-passwd")
        || k.ends_with("_pass")
        || k.ends_with("-pass")
        || k == "secret"
        || k.ends_with("_secret")
        || k.ends_with("-secret")
        || k == "hdnea"
        || k == "policy"
        || k == "credential"
        || k == "x-amz-credential"
        || k == "cookie"
        || k == "session"
        || k == "session_id"
        || k == "jwt"
}
