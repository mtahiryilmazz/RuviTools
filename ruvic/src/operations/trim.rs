use std::path::PathBuf;

use crate::backend::MediaBackend;
use crate::config::RuvicConfig;
use crate::domain::conversion_job::{ConversionJob, OperationKind, OperationResult};
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::ContainerFormat;
use crate::error::RuviResult;
use crate::operations::run_job_pipeline;

#[derive(Debug, Clone)]
pub struct TrimRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub start: String,
    pub end: Option<String>,
    pub duration: Option<f64>,
    pub reencode: bool,
    pub overwrite: bool,
}

pub async fn execute_trim(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    req: TrimRequest,
    show_progress: bool,
) -> RuviResult<OperationResult> {
    let container = ContainerFormat::from_path(&req.output)?;
    let overwrite = if req.overwrite || config.processing.overwrite {
        OverwritePolicy::Overwrite
    } else {
        OverwritePolicy::Reject
    };

    let job = ConversionJob {
        kind: OperationKind::Trim,
        input: req.input,
        output: req.output,
        container,
        overwrite,
        video_codec: None,
        audio_codec: None,
        quality: None,
        speed_preset: None,
        pixel_format: None,
        audio_bitrate: None,
        max_height: None,
        faststart: container == ContainerFormat::Mp4,
        threads: config.processing.threads,
        hwaccel: HwAccelMode::None,
        video_stream: None,
        audio_stream: None,
        subtitle_mode: SubtitleMode::Copy,
        start_time: Some(req.start),
        end_time: req.end,
        duration_seconds: req.duration,
        thumbnail_at: None,
        reencode: req.reencode,
    };

    run_job_pipeline(backend, job, show_progress).await
}
