use std::path::PathBuf;

use crate::backend::{MediaBackend, ProgressSink};
use crate::config::RuvicConfig;
use crate::domain::conversion_job::{ConversionJob, OperationKind, OperationResult};
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::{AudioCodec, ContainerFormat};
use crate::error::RuviResult;
use crate::operations::run_job_pipeline;

#[derive(Debug, Clone)]
pub struct ExtractAudioRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub to: Option<String>,
    pub copy: bool,
    pub overwrite: bool,
    pub audio_stream: Option<usize>,
    pub bitrate: Option<String>,
    pub start: Option<String>,
    pub duration: Option<f64>,
}

pub async fn execute_extract_audio(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    req: ExtractAudioRequest,
    show_progress: bool,
) -> RuviResult<OperationResult> {
    let job = build_job(config, req)?;
    run_job_pipeline(backend, job, show_progress).await
}

pub async fn execute_extract_audio_with_sink(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    req: ExtractAudioRequest,
    progress: &dyn ProgressSink,
) -> RuviResult<OperationResult> {
    let job = build_job(config, req)?;
    crate::operations::run_job_pipeline_with_sink(backend, job, progress).await
}

fn build_job(config: &RuvicConfig, req: ExtractAudioRequest) -> RuviResult<ConversionJob> {
    let container = match req.to {
        Some(ref fmt) => ContainerFormat::from_name(fmt)?,
        None => ContainerFormat::from_path(&req.output)?,
    };
    let audio_codec = if req.copy {
        Some(AudioCodec::Copy)
    } else {
        None
    };
    let overwrite = if req.overwrite || config.processing.overwrite {
        OverwritePolicy::Overwrite
    } else {
        OverwritePolicy::Reject
    };

    Ok(ConversionJob {
        kind: OperationKind::ExtractAudio,
        input: req.input,
        secondary_input: None,
        output: req.output,
        container,
        overwrite,
        video_codec: None,
        audio_codec,
        quality: None,
        speed_preset: None,
        pixel_format: None,
        audio_bitrate: req.bitrate.or_else(|| Some("192k".to_string())),
        max_height: None,
        faststart: false,
        threads: config.processing.threads,
        hwaccel: HwAccelMode::None,
        video_stream: None,
        audio_stream: req.audio_stream,
        subtitle_mode: SubtitleMode::None,
        start_time: req.start,
        end_time: None,
        duration_seconds: req.duration,
        thumbnail_at: None,
        reencode: !req.copy,
    })
}
