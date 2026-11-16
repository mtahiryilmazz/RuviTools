use std::path::PathBuf;

use crate::backend::MediaBackend;
use crate::config::RuvicConfig;
use crate::domain::conversion_job::{ConversionJob, OperationKind, OperationResult};
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::ContainerFormat;
use crate::error::RuviResult;
use crate::operations::run_job_pipeline;

#[derive(Debug, Clone)]
pub struct RemuxRequest {
    pub input: PathBuf,
    pub secondary_input: Option<PathBuf>,
    pub output: PathBuf,
    pub overwrite: bool,
    pub video_stream: Option<usize>,
    pub audio_stream: Option<usize>,
}

pub async fn execute_remux(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    req: RemuxRequest,
    show_progress: bool,
) -> RuviResult<OperationResult> {
    let container = ContainerFormat::from_path(&req.output)?;
    let overwrite = if req.overwrite || config.processing.overwrite {
        OverwritePolicy::Overwrite
    } else {
        OverwritePolicy::Reject
    };

    let job = ConversionJob {
        kind: OperationKind::Remux,
        input: req.input,
        secondary_input: req.secondary_input,
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
        video_stream: req.video_stream,
        audio_stream: req.audio_stream,
        subtitle_mode: SubtitleMode::Copy,
        start_time: None,
        end_time: None,
        duration_seconds: None,
        thumbnail_at: None,
        reencode: false,
    };

    run_job_pipeline(backend, job, show_progress).await
}
