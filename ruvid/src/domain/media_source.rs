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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub language: String,
    pub name: Option<String>,
    pub url: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaSource {
    pub url: String,
    pub resolved_url: String,
    pub kind: SourceKind,
    pub title: String,
    pub default_ext: String,
    pub streams: Vec<MediaStream>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
}
