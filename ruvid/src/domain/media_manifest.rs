use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

pub const MANIFEST_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuviManifest {
    pub schema_version: String,
    pub producer: ManifestProducer,
    pub source: ManifestSource,
    pub output: ManifestOutput,
    pub streams: Vec<ManifestStream>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtitles: Vec<ManifestSubtitle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dual_stream: Option<DualStreamHandoff>,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DualStreamHandoff {
    pub video_path: String,
    pub audio_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestProducer {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestSource {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestOutput {
    pub path: String,
    pub container: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestStream {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestSubtitle {
    pub language: String,
    pub path: String,
    pub format: String,
}

use crate::domain::media_source::MediaSource;
use crate::domain::stream::StreamPayload;

/// Redacts credentials and sensitive query parameters (`token`, `sig`, `key`, `auth`, `secret`, `hdnea`, `policy`, etc.)
/// before writing URLs to manifests, JSON output, or logs.
pub fn redact_sensitive_url(raw: &str) -> String {
    let Ok(mut parsed) = Url::parse(raw) else {
        return raw.to_string();
    };
    if !parsed.username().is_empty() {
        let _ = parsed.set_username("REDACTED");
    }
    if parsed.password().is_some() {
        let _ = parsed.set_password(Some("REDACTED"));
    }

    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            let lk = k.to_ascii_lowercase();
            let is_sensitive = [
                "token",
                "sig",
                "signature",
                "key",
                "auth",
                "secret",
                "pass",
                "pwd",
                "hdnea",
                "policy",
                "credential",
                "cookie",
                "expires",
                "session",
                "jwt",
                "bearer",
                "x-amz-",
            ]
            .iter()
            .any(|marker| lk.contains(marker))
                || lk == "sid";
            if is_sensitive {
                (k.into_owned(), "REDACTED".to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();

    if !pairs.is_empty() {
        parsed.query_pairs_mut().clear().extend_pairs(pairs);
    }
    parsed.to_string()
}

/// Redacts all URL credentials and sensitive query tokens inside an arbitrary text message (e.g. errors/logs).
pub fn redact_urls_in_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(pos) = find_next_url_start(rest) {
        out.push_str(&rest[..pos]);
        let candidate_slice = &rest[pos..];
        let end = candidate_slice
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']'))
            .unwrap_or(candidate_slice.len());
        let raw_token = &candidate_slice[..end];
        let trimmed = raw_token.trim_end_matches(['.', ',', ';', ':']);
        let trailing = &raw_token[trimmed.len()..];
        out.push_str(&redact_sensitive_url(trimmed));
        out.push_str(trailing);
        rest = &candidate_slice[end..];
    }
    out.push_str(rest);
    out
}

fn find_next_url_start(haystack: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    ["https://", "http://", "ftp://", "socks5://", "socks5h://"]
        .iter()
        .filter_map(|scheme| lower.find(scheme))
        .min()
}

/// Redacts all URLs inside a [`MediaSource`] before emitting it in `info` output or logs.
pub fn redact_media_source(source: &mut MediaSource) {
    source.url = redact_sensitive_url(&source.url);
    source.resolved_url = redact_sensitive_url(&source.resolved_url);
    for stream in &mut source.streams {
        match &mut stream.payload {
            StreamPayload::DirectHttp { url, .. } => {
                *url = redact_sensitive_url(url);
            }
            StreamPayload::Hls {
                playlist_url,
                init_segment_url,
                segments,
                ..
            } => {
                *playlist_url = redact_sensitive_url(playlist_url);
                if let Some(init) = init_segment_url.as_mut() {
                    *init = redact_sensitive_url(init);
                }
                for seg in segments {
                    seg.url = redact_sensitive_url(&seg.url);
                }
            }
            StreamPayload::Dash {
                mpd_url,
                init_segment_url,
                segments,
            } => {
                *mpd_url = redact_sensitive_url(mpd_url);
                if let Some(init) = init_segment_url.as_mut() {
                    *init = redact_sensitive_url(init);
                }
                for seg in segments {
                    seg.url = redact_sensitive_url(&seg.url);
                }
            }
        }
    }
    for sub in &mut source.subtitles {
        sub.url = redact_sensitive_url(&sub.url);
    }
}

pub fn utc_now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

// Howard Hinnant civil-from-days algorithm (UTC epoch 1970-01-01)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}
