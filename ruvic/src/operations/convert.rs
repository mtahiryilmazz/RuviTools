use std::path::{Path, PathBuf};

use crate::backend::MediaBackend;
use crate::config::RuvicConfig;
use crate::domain::conversion_job::{ConversionJob, OperationKind, OperationResult};
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::{AudioCodec, ContainerFormat, VideoCodec};
use crate::error::RuviResult;
use crate::operations::run_job_pipeline;
use crate::presets::find_preset;

#[derive(Debug, Clone)]
pub struct ConvertRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub quality: Option<u8>,
    pub speed: Option<String>,
    pub preset: Option<String>,
    pub overwrite: bool,
    pub threads: Option<u32>,
    pub hwaccel: Option<String>,
    pub video_stream: Option<usize>,
    pub audio_stream: Option<usize>,
    pub subtitle: Option<String>,
}

pub async fn execute_convert(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    user_presets_dir: Option<&Path>,
    req: ConvertRequest,
    show_progress: bool,
) -> RuviResult<OperationResult> {
    let preset_name = req
        .preset
        .as_deref()
        .or(config.processing.default_preset.as_deref());
    let loaded_preset = match preset_name {
        Some(name) => Some(find_preset(name, user_presets_dir)?),
        None => None,
    };

    let container = if req.output.extension().is_some() {
        ContainerFormat::from_path(&req.output)?
    } else if let Some(ref p) = loaded_preset {
        ContainerFormat::from_name(&p.container.format)?
    } else {
        ContainerFormat::from_path(&req.output)?
    };

    let video_codec = match req.video {
        Some(ref v) => Some(VideoCodec::from_name(v)?),
        None => match loaded_preset.as_ref().and_then(|p| p.video.as_ref()) {
            Some(pv) => Some(VideoCodec::from_name(&pv.codec)?),
            None => None,
        },
    };

    let audio_codec = match req.audio {
        Some(ref a) => Some(AudioCodec::from_name(a)?),
        None => match loaded_preset.as_ref().and_then(|p| p.audio.as_ref()) {
            Some(pa) => Some(AudioCodec::from_name(&pa.codec)?),
            None => None,
        },
    };

    let quality = req.quality.or_else(|| {
        loaded_preset
            .as_ref()
            .and_then(|p| p.video.as_ref())
            .and_then(|v| v.quality)
    });
    let speed_preset = req.speed.or_else(|| {
        loaded_preset
            .as_ref()
            .and_then(|p| p.video.as_ref())
            .and_then(|v| v.preset.clone())
    });
    let pixel_format = loaded_preset
        .as_ref()
        .and_then(|p| p.video.as_ref())
        .and_then(|v| v.pixel_format.clone());
    let max_height = loaded_preset
        .as_ref()
        .and_then(|p| p.video.as_ref())
        .and_then(|v| v.max_height);
    let audio_bitrate = loaded_preset
        .as_ref()
        .and_then(|p| p.audio.as_ref())
        .and_then(|a| a.bitrate.clone());
    let faststart = loaded_preset
        .as_ref()
        .map(|p| p.container.faststart)
        .unwrap_or(container == ContainerFormat::Mp4);

    let hwaccel =
        HwAccelMode::from_name(req.hwaccel.as_deref().unwrap_or(&config.processing.hwaccel))?;
    let subtitle_mode = match req.subtitle {
        Some(ref s) => SubtitleMode::from_name(s)?,
        None => SubtitleMode::None,
    };
    let overwrite = if req.overwrite || config.processing.overwrite {
        OverwritePolicy::Overwrite
    } else {
        OverwritePolicy::Reject
    };

    let job = ConversionJob {
        kind: OperationKind::Convert,
        input: req.input,
        output: req.output,
        container,
        overwrite,
        video_codec,
        audio_codec,
        quality,
        speed_preset,
        pixel_format,
        audio_bitrate,
        max_height,
        faststart,
        threads: req.threads.or(config.processing.threads),
        hwaccel,
        video_stream: req.video_stream,
        audio_stream: req.audio_stream,
        subtitle_mode,
        start_time: None,
        end_time: None,
        duration_seconds: None,
        thumbnail_at: None,
        reencode: false,
    };

    run_job_pipeline(backend, job, show_progress).await
}
