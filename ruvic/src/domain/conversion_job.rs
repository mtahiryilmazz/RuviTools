use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::media_info::MediaInfo;
use crate::domain::output_spec::{HwAccelMode, OverwritePolicy, SubtitleMode};
use crate::domain::stream::{AudioCodec, ContainerFormat, VideoCodec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Convert,
    Remux,
    ExtractAudio,
    Trim,
    Thumbnail,
}

#[derive(Debug, Clone)]
pub struct ConversionJob {
    pub kind: OperationKind,
    pub input: PathBuf,
    pub output: PathBuf,
    pub container: ContainerFormat,
    pub overwrite: OverwritePolicy,
    pub video_codec: Option<VideoCodec>,
    pub audio_codec: Option<AudioCodec>,
    pub quality: Option<u8>,
    pub speed_preset: Option<String>,
    pub pixel_format: Option<String>,
    pub audio_bitrate: Option<String>,
    pub max_height: Option<u32>,
    pub faststart: bool,
    pub threads: Option<u32>,
    pub hwaccel: HwAccelMode,
    pub video_stream: Option<usize>,
    pub audio_stream: Option<usize>,
    pub subtitle_mode: SubtitleMode,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_seconds: Option<f64>,
    pub thumbnail_at: Option<String>,
    pub reencode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionReport {
    pub exit_code: i32,
    pub stderr_tail: Vec<String>,
    pub selected_video_encoder: Option<String>,
    pub selected_audio_encoder: Option<String>,
    pub hwaccel_requested: String,
    pub hwaccel_used: Option<String>,
    pub hwaccel_fallback_reason: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationResult {
    pub ok: bool,
    pub operation: OperationKind,
    pub input: String,
    pub output: String,
    pub container: String,
    pub output_size_bytes: u64,
    pub selected_video_encoder: Option<String>,
    pub selected_audio_encoder: Option<String>,
    pub hwaccel_used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hwaccel_fallback_reason: Option<String>,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_probe: Option<MediaInfo>,
}
