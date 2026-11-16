use std::path::PathBuf;

use crate::backend::MediaBackend;
use crate::config::RuvicConfig;
use crate::domain::conversion_job::{ConversionJob, OperationKind, OperationResult};
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::ContainerFormat;
use crate::error::RuviResult;
use crate::operations::run_job_pipeline;

#[derive(Debug, Clone)]
pub struct ThumbnailRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub at: String,
    pub overwrite: bool,
}

pub async fn execute_thumbnail(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    req: ThumbnailRequest,
    show_progress: bool,
) -> RuviResult<OperationResult> {
    let container = ContainerFormat::from_path(&req.output)?;
    let overwrite = if req.overwrite || config.processing.overwrite {
        OverwritePolicy::Overwrite
    } else {
        OverwritePolicy::Reject
    };

    let job = ConversionJob {
        kind: OperationKind::Thumbnail,
        input: req.input,
        secondary_input: None,
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
        faststart: false,
        threads: config.processing.threads,
        hwaccel: HwAccelMode::None,
        video_stream: None,
        audio_stream: None,
        subtitle_mode: SubtitleMode::None,
        start_time: None,
        end_time: None,
        duration_seconds: None,
        thumbnail_at: Some(req.at),
        reencode: false,
    };

    run_job_pipeline(backend, job, show_progress).await
}
