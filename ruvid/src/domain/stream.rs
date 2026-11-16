use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    VideoAudio,
    Video,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamPayload {
    DirectHttp {
        url: String,
        total_bytes: Option<u64>,
        accept_ranges: bool,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    Hls {
        playlist_url: String,
        init_segment_url: Option<String>,
        segments: Vec<MediaSegment>,
        is_vod: bool,
    },
    Dash {
        mpd_url: String,
        init_segment_url: Option<String>,
        segments: Vec<MediaSegment>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaSegment {
    pub index: usize,
    pub url: String,
    pub byte_range: Option<ByteRangeSpec>,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByteRangeSpec {
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaStream {
    pub id: String,
    pub kind: StreamKind,
    pub container: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bandwidth_bps: Option<u64>,
    pub codecs: Option<String>,
    pub frame_rate: Option<String>,
    pub language: Option<String>,
    pub payload: StreamPayload,
}
