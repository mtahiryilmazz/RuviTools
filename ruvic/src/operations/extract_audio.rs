use std::path::PathBuf;

use crate::backend::MediaBackend;
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
}

pub async fn execute_extract_audio(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    req: ExtractAudioRequest,
    show_progress: bool,
) -> RuviResult<OperationResult> {
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

    let job = ConversionJob {
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
        audio_bitrate: Some("192k".to_string()),
        max_height: None,
        faststart: false,
        threads: config.processing.threads,
        hwaccel: HwAccelMode::None,
        video_stream: None,
        audio_stream: req.audio_stream,
        subtitle_mode: SubtitleMode::None,
        start_time: None,
        end_time: None,
        duration_seconds: None,
        thumbnail_at: None,
        reencode: !req.copy,
    };

    run_job_pipeline(backend, job, show_progress).await
}
