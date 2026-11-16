use crate::backend::BackendCapabilities;
use crate::domain::output_spec::HwAccelMode;
use crate::domain::stream::{AudioCodec, VideoCodec};

#[derive(Debug, Clone)]
pub struct ResolvedEncoders {
    pub video_encoder: Option<String>,
    pub audio_encoder: Option<String>,
    pub hwaccel_input_flag: Option<String>,
    pub hwaccel_fallback_reason: Option<String>,
}

pub fn resolve_encoders(
    video: Option<VideoCodec>,
    audio: Option<AudioCodec>,
    hwaccel: HwAccelMode,
    caps: &BackendCapabilities,
) -> ResolvedEncoders {
    let mut hwaccel_input_flag = None;
    let mut fallback_reason = None;

    let video_encoder = video.map(|codec| {
        if codec == VideoCodec::Copy {
            return "copy".to_string();
        }
        match hwaccel {
            HwAccelMode::None => codec.default_ffmpeg_encoder().to_string(),
            HwAccelMode::Auto => {
                if let Some((enc, hw)) = try_auto_hw_encoder(codec, caps) {
                    hwaccel_input_flag = Some(hw.to_string());
                    enc.to_string()
                } else {
                    fallback_reason = Some(
                        "no hardware encoder detected for requested codec; using CPU encoder"
                            .to_string(),
                    );
                    codec.default_ffmpeg_encoder().to_string()
                }
            }
            mode => {
                if let Some((enc, hw)) = try_specific_hw_encoder(codec, mode, caps) {
                    hwaccel_input_flag = Some(hw.to_string());
                    enc.to_string()
                } else {
                    fallback_reason = Some(format!(
                        "requested hwaccel '{}' is unavailable for this codec/backend; falling back to CPU encoder",
                        mode.as_str()
                    ));
                    codec.default_ffmpeg_encoder().to_string()
                }
            }
        }
    });

    let audio_encoder = audio.map(|a| a.default_ffmpeg_encoder().to_string());

    ResolvedEncoders {
        video_encoder,
        audio_encoder,
        hwaccel_input_flag,
        hwaccel_fallback_reason: fallback_reason,
    }
}

fn try_auto_hw_encoder(
    codec: VideoCodec,
    caps: &BackendCapabilities,
) -> Option<(&'static str, &'static str)> {
    for mode in [
        HwAccelMode::Nvenc,
        HwAccelMode::Qsv,
        HwAccelMode::Amf,
        HwAccelMode::VideoToolbox,
    ] {
        if let Some(found) = try_specific_hw_encoder(codec, mode, caps) {
            return Some(found);
        }
    }
    None
}

fn try_specific_hw_encoder(
    codec: VideoCodec,
    mode: HwAccelMode,
    caps: &BackendCapabilities,
) -> Option<(&'static str, &'static str)> {
    let (encoder, hw_flag) = match (codec, mode) {
        (VideoCodec::H264, HwAccelMode::Nvenc) => ("h264_nvenc", "cuda"),
        (VideoCodec::H265, HwAccelMode::Nvenc) => ("hevc_nvenc", "cuda"),
        (VideoCodec::H264, HwAccelMode::Qsv) => ("h264_qsv", "qsv"),
        (VideoCodec::H265, HwAccelMode::Qsv) => ("hevc_qsv", "qsv"),
        (VideoCodec::H264, HwAccelMode::Amf) => ("h264_amf", "d3d11va"),
        (VideoCodec::H265, HwAccelMode::Amf) => ("hevc_amf", "d3d11va"),
        (VideoCodec::H264, HwAccelMode::VideoToolbox) => ("h264_videotoolbox", "videotoolbox"),
        (VideoCodec::H265, HwAccelMode::VideoToolbox) => ("hevc_videotoolbox", "videotoolbox"),
        _ => return None,
    };

    if caps.has_encoder(encoder) {
        Some((encoder, hw_flag))
    } else {
        None
    }
}
