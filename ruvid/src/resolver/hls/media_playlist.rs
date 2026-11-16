use url::Url;

use crate::domain::stream::{ByteRangeSpec, MediaSegment, MediaStream, StreamKind, StreamPayload};
use crate::error::{RuviError, RuviResult};
use crate::resolver::MAX_PLAYLIST_SEGMENTS;
use crate::resolver::hls::parser::parse_attribute_list;

#[derive(Debug, Clone)]
pub struct ParsedMediaPlaylist {
    pub init_segment_url: Option<String>,
    pub segments: Vec<MediaSegment>,
    pub is_vod: bool,
    pub container_ext: String,
}

pub fn parse_media_playlist(base_url: &Url, body: &str) -> RuviResult<ParsedMediaPlaylist> {
    let mut segments = Vec::new();
    let mut init_segment_url = None;
    let mut is_vod = false;
    let mut pending_duration: Option<u64> = None;
    let mut pending_range: Option<ByteRangeSpec> = None;
    let mut prev_range_end: u64 = 0;

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let attrs = parse_attribute_list(rest);
            let method = attrs.get("METHOD").map(String::as_str).unwrap_or("NONE");
            if !method.eq_ignore_ascii_case("NONE") {
                return Err(RuviError::Unsupported(format!(
                    "encrypted or DRM-protected HLS stream (METHOD={method}) is not supported"
                )));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            let attrs = parse_attribute_list(rest);
            if let Some(uri) = attrs.get("URI") {
                let joined = base_url.join(uri).map_err(|err| {
                    RuviError::Unsupported(format!("invalid #EXT-X-MAP URI '{uri}': {err}"))
                })?;
                init_segment_url = Some(joined.to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let dur_str = rest.split(',').next().unwrap_or("0").trim();
            pending_duration = dur_str.parse::<f64>().ok().map(|d| d.round() as u64);
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_range = parse_byterange(rest, prev_range_end);
            if let Some(r) = pending_range {
                prev_range_end = r.offset + r.length;
            }
            continue;
        }
        if line == "#EXT-X-ENDLIST" {
            is_vod = true;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        if segments.len() >= MAX_PLAYLIST_SEGMENTS {
            return Err(RuviError::Unsupported(format!(
                "HLS playlist exceeds maximum segment limit ({MAX_PLAYLIST_SEGMENTS})"
            )));
        }

        let seg_url = base_url.join(line).map_err(|err| {
            RuviError::Unsupported(format!("invalid HLS segment URI '{line}': {err}"))
        })?;
        segments.push(MediaSegment {
            index: segments.len(),
            url: seg_url.to_string(),
            byte_range: pending_range.take(),
            duration_seconds: pending_duration.take(),
        });
    }

    if segments.is_empty() {
        return Err(RuviError::Unsupported(
            "HLS media playlist contains no segments".to_string(),
        ));
    }
    if !is_vod {
        return Err(RuviError::Unsupported(
            "live HLS playlists without #EXT-X-ENDLIST are not supported in VOD mode".to_string(),
        ));
    }

    let container_ext = if init_segment_url.is_some()
        || segments
            .first()
            .is_some_and(|s| s.url.to_ascii_lowercase().contains(".m4s"))
    {
        "mp4".to_string()
    } else {
        "ts".to_string()
    };

    Ok(ParsedMediaPlaylist {
        init_segment_url,
        segments,
        is_vod,
        container_ext,
    })
}

pub fn build_single_stream(playlist_url: &Url, parsed: &ParsedMediaPlaylist) -> MediaStream {
    MediaStream {
        id: "hls-0".to_string(),
        kind: StreamKind::VideoAudio,
        container: parsed.container_ext.clone(),
        width: None,
        height: None,
        bandwidth_bps: None,
        codecs: None,
        frame_rate: None,
        language: None,
        payload: StreamPayload::Hls {
            playlist_url: playlist_url.to_string(),
            init_segment_url: parsed.init_segment_url.clone(),
            segments: parsed.segments.clone(),
            is_vod: parsed.is_vod,
        },
    }
}

fn parse_byterange(raw: &str, default_offset: u64) -> Option<ByteRangeSpec> {
    let trimmed = raw.trim();
    if let Some((len_str, off_str)) = trimmed.split_once('@') {
        let length = len_str.trim().parse::<u64>().ok()?;
        let offset = off_str.trim().parse::<u64>().ok()?;
        Some(ByteRangeSpec { offset, length })
    } else {
        let length = trimmed.parse::<u64>().ok()?;
        Some(ByteRangeSpec {
            offset: default_offset,
            length,
        })
    }
}
