use std::path::PathBuf;

use crate::backend::BackendCapabilities;
use crate::domain::conversion_job::{ConversionJob, OperationKind};
use crate::domain::media_info::MediaInfo;
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::{AudioCodec, ContainerFormat, StreamKind, VideoCodec};
use crate::error::{RuviError, RuviResult};
use crate::planner::codec_selection::resolve_encoders;
use crate::planner::validation::{
    is_allowed_speed_preset, is_valid_bitrate, is_valid_pixel_format,
    validate_audio_copy_compatibility, validate_io_paths, validate_quality,
    validate_remux_compatibility, validate_selected_streams, validate_target_codecs,
    validate_timestamp, validate_video_copy_compatibility,
};

#[derive(Debug, Clone)]
pub struct ConversionPlan {
    pub operation: OperationKind,
    pub input: PathBuf,
    pub temp_output: PathBuf,
    pub final_output: PathBuf,
    pub overwrite: OverwritePolicy,
    pub container: ContainerFormat,
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
    pub video_encoder: Option<String>,
    pub audio_encoder: Option<String>,
    pub quality: Option<u8>,
    pub speed_preset: Option<String>,
    pub pixel_format: Option<String>,
    pub audio_bitrate: Option<String>,
    pub max_height: Option<u32>,
    pub faststart: bool,
    pub threads: Option<u32>,
    pub hwaccel_requested: HwAccelMode,
    pub hwaccel_input_flag: Option<String>,
    pub hwaccel_fallback_reason: Option<String>,
    pub video_stream: Option<usize>,
    pub audio_stream: Option<usize>,
    pub subtitle_mode: SubtitleMode,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_seconds: Option<f64>,
    pub thumbnail_at: Option<String>,
    pub reencode: bool,
}

pub fn plan_conversion(
    job: ConversionJob,
    probe_info: &MediaInfo,
    caps: &BackendCapabilities,
    temp_output: PathBuf,
) -> RuviResult<ConversionPlan> {
    validate_io_paths(&job.input, &job.output, job.overwrite)?;
    validate_selected_streams(probe_info, job.video_stream, job.audio_stream)?;

    let has_video = probe_info
        .streams
        .iter()
        .any(|s| s.kind == StreamKind::Video);
    let has_audio = probe_info
        .streams
        .iter()
        .any(|s| s.kind == StreamKind::Audio);

    let (eff_video, eff_audio) = match job.kind {
        OperationKind::Remux => {
            validate_remux_compatibility(
                probe_info,
                job.container,
                job.video_stream,
                job.audio_stream,
            )?;
            (
                has_video.then_some(VideoCodec::Copy),
                has_audio.then_some(AudioCodec::Copy),
            )
        }
        OperationKind::ExtractAudio => {
            if !has_audio {
                return Err(RuviError::Unsupported(
                    "input media contains no audio stream".to_string(),
                ));
            }
            let audio = job
                .audio_codec
                .unwrap_or_else(|| default_audio_for_container(job.container));
            validate_target_codecs(job.container, None, Some(audio))?;
            if audio == AudioCodec::Copy {
                validate_audio_copy_compatibility(probe_info, job.container, job.audio_stream)?;
            }
            (None, Some(audio))
        }
        OperationKind::Thumbnail => {
            if !has_video {
                return Err(RuviError::Unsupported(
                    "input media contains no video stream for thumbnail extraction".to_string(),
                ));
            }
            if !job.container.is_image() {
                return Err(RuviError::Unsupported(format!(
                    "thumbnail output must be an image container (jpg/png/webp), got '{}'",
                    job.container.as_str()
                )));
            }
            let at = job.thumbnail_at.as_deref().ok_or_else(|| {
                RuviError::InvalidArgument("thumbnail requires --at <TIME>".to_string())
            })?;
            validate_timestamp(at, "--at")?;
            (None, None)
        }
        OperationKind::Trim => {
            let start = job.start_time.as_deref().ok_or_else(|| {
                RuviError::InvalidArgument("trim requires --start <TIME>".to_string())
            })?;
            validate_timestamp(start, "--start")?;
            if job.end_time.is_none() && job.duration_seconds.is_none() {
                return Err(RuviError::InvalidArgument(
                    "trim requires either --end <TIME> or --duration <SECONDS>".to_string(),
                ));
            }
            if job.end_time.is_some() && job.duration_seconds.is_some() {
                return Err(RuviError::InvalidArgument(
                    "trim cannot combine --end and --duration".to_string(),
                ));
            }
            if let Some(ref end) = job.end_time {
                validate_timestamp(end, "--end")?;
                let start_s = crate::planner::validation::parse_timestamp_seconds(start);
                let end_s = crate::planner::validation::parse_timestamp_seconds(end);
                if let (Some(s), Some(e)) = (start_s, end_s)
                    && e <= s
                {
                    return Err(RuviError::InvalidArgument(
                        "--end timestamp must be greater than --start".to_string(),
                    ));
                }
            }
            if let Some(dur) = job.duration_seconds
                && (!dur.is_finite() || dur <= 0.0)
            {
                return Err(RuviError::InvalidArgument(
                    "--duration must be a finite number greater than 0".to_string(),
                ));
            }
            if job.reencode {
                let v = has_video.then(|| {
                    job.video_codec
                        .unwrap_or_else(|| default_video_for_container(job.container))
                });
                let a = has_audio.then(|| {
                    job.audio_codec
                        .unwrap_or_else(|| default_audio_for_container(job.container))
                });
                validate_target_codecs(job.container, v, a)?;
                (v, a)
            } else {
                validate_remux_compatibility(
                    probe_info,
                    job.container,
                    job.video_stream,
                    job.audio_stream,
                )?;
                (
                    has_video.then_some(VideoCodec::Copy),
                    has_audio.then_some(AudioCodec::Copy),
                )
            }
        }
        OperationKind::Convert => {
            let v = if job.container.is_audio_only() {
                None
            } else if has_video {
                Some(
                    job.video_codec
                        .unwrap_or_else(|| default_video_for_container(job.container)),
                )
            } else {
                None
            };
            let a = if has_audio {
                Some(
                    job.audio_codec
                        .unwrap_or_else(|| default_audio_for_container(job.container)),
                )
            } else {
                None
            };
            if v.is_none() && a.is_none() {
                return Err(RuviError::Unsupported(
                    "input media contains no video or audio streams to convert".to_string(),
                ));
            }
            validate_target_codecs(job.container, v, a)?;
            if v == Some(VideoCodec::Copy) {
                validate_video_copy_compatibility(probe_info, job.container, job.video_stream)?;
            }
            if a == Some(AudioCodec::Copy) {
                validate_audio_copy_compatibility(probe_info, job.container, job.audio_stream)?;
            }
            (v, a)
        }
    };

    if let Some(q) = job.quality {
        validate_quality(q, eff_video)?;
    }
    if let Some(ref speed) = job.speed_preset
        && !is_allowed_speed_preset(speed)
    {
        return Err(RuviError::InvalidArgument(format!(
            "invalid encoder speed preset '{speed}'"
        )));
    }
    if let Some(ref pix_fmt) = job.pixel_format
        && !is_valid_pixel_format(pix_fmt)
    {
        return Err(RuviError::InvalidArgument(format!(
            "invalid pixel format '{pix_fmt}'"
        )));
    }
    if let Some(ref bitrate) = job.audio_bitrate
        && !is_valid_bitrate(bitrate)
    {
        return Err(RuviError::InvalidArgument(format!(
            "invalid audio bitrate '{bitrate}'"
        )));
    }
    if job.max_height == Some(0) {
        return Err(RuviError::InvalidArgument(
            "max_height must be greater than 0".to_string(),
        ));
    }

    let resolved = resolve_encoders(eff_video, eff_audio, job.hwaccel, caps);

    Ok(ConversionPlan {
        operation: job.kind,
        input: job.input,
        temp_output,
        final_output: job.output,
        overwrite: job.overwrite,
        container: job.container,
        video_codec: eff_video,
        audio_codec: eff_audio,
        video_encoder: resolved.video_encoder,
        audio_encoder: resolved.audio_encoder,
        quality: job.quality,
        speed_preset: job.speed_preset,
        pixel_format: job.pixel_format,
        audio_bitrate: job.audio_bitrate,
        max_height: job.max_height,
        faststart: job.faststart,
        threads: job.threads,
        hwaccel_requested: job.hwaccel,
        hwaccel_input_flag: resolved.hwaccel_input_flag,
        hwaccel_fallback_reason: resolved.hwaccel_fallback_reason,
        video_stream: job.video_stream,
        audio_stream: job.audio_stream,
        subtitle_mode: job.subtitle_mode,
        start_time: job.start_time,
        end_time: job.end_time,
        duration_seconds: job.duration_seconds,
        thumbnail_at: job.thumbnail_at,
        reencode: job.reencode,
    })
}

fn default_video_for_container(container: ContainerFormat) -> VideoCodec {
    match container {
        ContainerFormat::Webm => VideoCodec::Vp9,
        _ => VideoCodec::H264,
    }
}

fn default_audio_for_container(container: ContainerFormat) -> AudioCodec {
    match container {
        ContainerFormat::Mp3 => AudioCodec::Mp3,
        ContainerFormat::Flac => AudioCodec::Flac,
        ContainerFormat::Wav => AudioCodec::PcmS16Le,
        ContainerFormat::Webm | ContainerFormat::Ogg => AudioCodec::Opus,
        _ => AudioCodec::Aac,
    }
}
