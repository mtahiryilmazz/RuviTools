use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::media_source::MediaSource;
use crate::domain::stream::MediaStream;

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub url: String,
    pub output_template: Option<PathBuf>,
    pub format_spec: String,
    pub jobs: usize,
    pub resume: bool,
    pub overwrite: bool,
    pub limit_rate: Option<String>,
    pub retry_count: u32,
    pub timeout_seconds: u64,
    pub proxy: Option<String>,
    pub headers: Vec<(String, String)>,
    pub write_manifest: Option<PathBuf>,
    pub to_stdout: bool,
    pub subtitles: bool,
    pub subtitles_lang: Option<String>,
    pub sub_format: String,
    pub auto_subs: bool,
    pub audio_lang: Option<String>,
    pub split_chapters: bool,
    pub clip: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_seconds: Option<f64>,
    pub allow_private_net: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadPlan {
    pub source: MediaSource,
    pub stream: MediaStream,
    pub output_path: PathBuf,
    pub jobs: usize,
    pub resume: bool,
    pub overwrite: bool,
    pub rate_limit_bps: Option<u64>,
    pub retry_count: u32,
    pub timeout_seconds: u64,
    pub to_stdout: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubtitleDownload {
    pub language: String,
    pub path: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub ok: bool,
    pub url: String,
    pub title: String,
    pub stream_id: String,
    pub output_path: String,
    pub bytes_downloaded: u64,
    pub resumed: bool,
    pub chunks_total: usize,
    pub chunks_resumed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtitles: Vec<SubtitleDownload>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistItemError {
    pub index: usize,
    pub url: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistReport {
    pub ok: bool,
    pub total_selected: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub items: Vec<DownloadOutcome>,
    pub errors: Vec<PlaylistItemError>,
}
