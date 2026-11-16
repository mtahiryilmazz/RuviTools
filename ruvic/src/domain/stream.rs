use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Data,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamInfo {
    pub index: usize,
    pub kind: StreamKind,
    pub codec_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub bit_rate: Option<u64>,
    pub language: Option<String>,
    pub pixel_format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerFormat {
    Mp4,
    Mkv,
    Webm,
    Mov,
    Ts,
    M4a,
    Mp3,
    Flac,
    Aac,
    Wav,
    Ogg,
    Jpg,
    Png,
    Webp,
}

impl ContainerFormat {
    pub fn from_name(name: &str) -> RuviResult<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "mp4" | "m4v" => Ok(Self::Mp4),
            "mkv" | "matroska" => Ok(Self::Mkv),
            "webm" => Ok(Self::Webm),
            "mov" => Ok(Self::Mov),
            "ts" | "mpegts" => Ok(Self::Ts),
            "m4a" => Ok(Self::M4a),
            "mp3" => Ok(Self::Mp3),
            "flac" => Ok(Self::Flac),
            "aac" | "adts" => Ok(Self::Aac),
            "wav" => Ok(Self::Wav),
            "ogg" | "oga" => Ok(Self::Ogg),
            "jpg" | "jpeg" => Ok(Self::Jpg),
            "png" => Ok(Self::Png),
            "webp" => Ok(Self::Webp),
            other => Err(RuviError::Unsupported(format!(
                "unsupported container format '{other}'"
            ))),
        }
    }

    pub fn from_path(path: &Path) -> RuviResult<Self> {
        let ext = path.extension().and_then(|s| s.to_str()).ok_or_else(|| {
            RuviError::InvalidArgument(format!(
                "cannot determine container format from output path without extension: {}",
                path.display()
            ))
        })?;
        Self::from_name(ext)
    }

    pub fn ffmpeg_format(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "matroska",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::Ts => "mpegts",
            Self::M4a => "ipod",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Aac => "adts",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
            Self::Jpg | Self::Png | Self::Webp => "image2",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::Ts => "ts",
            Self::M4a => "m4a",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Aac => "aac",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }

    pub fn is_audio_only(self) -> bool {
        matches!(
            self,
            Self::M4a | Self::Mp3 | Self::Flac | Self::Aac | Self::Wav | Self::Ogg
        )
    }

    pub fn is_image(self) -> bool {
        matches!(self, Self::Jpg | Self::Png | Self::Webp)
    }

    pub fn supports_video_codec(self, codec: VideoCodec) -> bool {
        if codec == VideoCodec::Copy {
            return !self.is_audio_only() && !self.is_image();
        }
        match self {
            Self::Mp4 | Self::Mov => {
                matches!(codec, VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Av1)
            }
            Self::Mkv => matches!(
                codec,
                VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Vp9 | VideoCodec::Av1
            ),
            Self::Webm => matches!(codec, VideoCodec::Vp9 | VideoCodec::Av1),
            Self::Ts => matches!(codec, VideoCodec::H264 | VideoCodec::H265),
            Self::M4a | Self::Mp3 | Self::Flac | Self::Aac | Self::Wav | Self::Ogg => false,
            Self::Jpg | Self::Png | Self::Webp => false,
        }
    }

    pub fn supports_audio_codec(self, codec: AudioCodec) -> bool {
        if codec == AudioCodec::Copy {
            return !self.is_image();
        }
        match self {
            Self::Mp4 | Self::Mov => {
                matches!(codec, AudioCodec::Aac | AudioCodec::Mp3 | AudioCodec::Opus)
            }
            Self::Mkv => true,
            Self::Webm => matches!(codec, AudioCodec::Opus | AudioCodec::Vorbis),
            Self::Ts => matches!(codec, AudioCodec::Aac | AudioCodec::Mp3),
            Self::M4a | Self::Aac => matches!(codec, AudioCodec::Aac),
            Self::Mp3 => matches!(codec, AudioCodec::Mp3),
            Self::Flac => matches!(codec, AudioCodec::Flac),
            Self::Wav => matches!(codec, AudioCodec::PcmS16Le),
            Self::Ogg => matches!(codec, AudioCodec::Vorbis | AudioCodec::Opus),
            Self::Jpg | Self::Png | Self::Webp => false,
        }
    }

    pub fn supports_raw_video_codec_name(self, raw_codec: &str) -> bool {
        if let Ok(parsed) = VideoCodec::from_name(raw_codec) {
            return self.supports_video_codec(parsed);
        }
        // Unknown codec names are permitted in Matroska; rejected for strict containers
        matches!(self, Self::Mkv)
    }

    pub fn supports_raw_audio_codec_name(self, raw_codec: &str) -> bool {
        if let Ok(parsed) = AudioCodec::from_name(raw_codec) {
            return self.supports_audio_codec(parsed);
        }
        matches!(self, Self::Mkv)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
    Av1,
    Copy,
}

impl VideoCodec {
    pub fn from_name(name: &str) -> RuviResult<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "h264" | "libx264" | "avc" | "avc1" | "h264_nvenc" | "h264_qsv" | "h264_amf"
            | "h264_videotoolbox" => Ok(Self::H264),
            "h265" | "hevc" | "libx265" | "hev1" | "hvc1" | "hevc_nvenc" | "hevc_qsv"
            | "hevc_amf" | "hevc_videotoolbox" => Ok(Self::H265),
            "vp9" | "libvpx-vp9" | "vp09" => Ok(Self::Vp9),
            "av1" | "libaom-av1" | "libsvtav1" | "av01" => Ok(Self::Av1),
            "copy" => Ok(Self::Copy),
            other => Err(RuviError::Unsupported(format!(
                "unsupported video codec '{other}'"
            ))),
        }
    }

    pub fn default_ffmpeg_encoder(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::H265 => "libx265",
            Self::Vp9 => "libvpx-vp9",
            Self::Av1 => "libaom-av1",
            Self::Copy => "copy",
        }
    }

    pub fn quality_help(self) -> &'static str {
        match self {
            Self::H264 => "CRF value (0-51, lower is better, typical 18-28)",
            Self::H265 => "CRF value (0-51, lower is better, typical 22-30)",
            Self::Vp9 => "CRF value (0-63, lower is better, typical 24-34)",
            Self::Av1 => "CRF value (0-63, lower is better, typical 26-36)",
            Self::Copy => "Quality is ignored when copying stream",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Flac,
    Vorbis,
    PcmS16Le,
    Copy,
}

impl AudioCodec {
    pub fn from_name(name: &str) -> RuviResult<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "aac" | "mp4a" => Ok(Self::Aac),
            "mp3" | "libmp3lame" => Ok(Self::Mp3),
            "opus" | "libopus" => Ok(Self::Opus),
            "flac" => Ok(Self::Flac),
            "vorbis" | "libvorbis" => Ok(Self::Vorbis),
            "pcm_s16le" | "wav" | "pcm" => Ok(Self::PcmS16Le),
            "copy" => Ok(Self::Copy),
            other => Err(RuviError::Unsupported(format!(
                "unsupported audio codec '{other}'"
            ))),
        }
    }

    pub fn default_ffmpeg_encoder(self) -> &'static str {
        match self {
            Self::Aac => "aac",
            Self::Mp3 => "libmp3lame",
            Self::Opus => "libopus",
            Self::Flac => "flac",
            Self::Vorbis => "libvorbis",
            Self::PcmS16Le => "pcm_s16le",
            Self::Copy => "copy",
        }
    }
}
