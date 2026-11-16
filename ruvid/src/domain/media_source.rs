use serde::{Deserialize, Serialize};

use crate::domain::stream::MediaStream;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    DirectHttp,
    HlsVod,
    DashVod,
    PlatformHtml,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub language: String,
    pub name: Option<String>,
    pub url: String,
    pub format: String,
    #[serde(default)]
    pub is_auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaSource {
    pub url: String,
    pub resolved_url: String,
    pub kind: SourceKind,
    pub title: String,
    pub default_ext: String,
    pub streams: Vec<MediaStream>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<Chapter>,
    #[serde(skip)]
    pub user_agent_override: Option<String>,
}
