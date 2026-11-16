use url::Url;

use crate::domain::media_source::{MediaSource, SourceKind, SubtitleTrack};
use crate::domain::stream::{MediaStream, StreamKind, StreamPayload};
use crate::error::{RuviError, RuviResult};
use crate::resolver::hls::derive_hls_title;
use crate::resolver::hls::parser::{parse_attribute_list, parse_resolution};

pub fn is_master_playlist(body: &str) -> bool {
    body.lines()
        .any(|l| l.trim_start().starts_with("#EXT-X-STREAM-INF"))
}

pub fn parse_master_playlist(
    original_url: &str,
    base_url: &Url,
    body: &str,
) -> RuviResult<MediaSource> {
    let mut streams = Vec::new();
    let mut subtitles = Vec::new();
    let mut pending_attrs: Option<std::collections::BTreeMap<String, String>> = None;
    let mut idx = 0usize;

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-SESSION-KEY:") {
            let attrs = parse_attribute_list(rest);
            let method = attrs.get("METHOD").map(String::as_str).unwrap_or("NONE");
            if !method.eq_ignore_ascii_case("NONE") {
                return Err(RuviError::Unsupported(format!(
                    "HLS session key encryption ({method}) / DRM is not supported"
                )));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA:") {
            let attrs = parse_attribute_list(rest);
            if attrs.get("TYPE").map(String::as_str) == Some("SUBTITLES")
                && let Some(uri) = attrs.get("URI")
                && let Ok(joined) = base_url.join(uri)
            {
                let lang = attrs
                    .get("LANGUAGE")
                    .cloned()
                    .unwrap_or_else(|| "und".to_string());
                let name = attrs.get("NAME").cloned();
                let format = if uri.to_ascii_lowercase().contains("srt") {
                    "srt"
                } else {
                    "vtt"
                };
                subtitles.push(SubtitleTrack {
                    language: lang,
                    name,
                    url: joined.to_string(),
                    format: format.to_string(),
                });
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending_attrs = Some(parse_attribute_list(rest));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        if let Some(attrs) = pending_attrs.take() {
            let variant_url = base_url.join(line).map_err(|err| {
                RuviError::Unsupported(format!("invalid HLS variant URI '{line}': {err}"))
            })?;
            let (width, height) = attrs
                .get("RESOLUTION")
                .map(|r| parse_resolution(r))
                .unwrap_or((None, None));
            let bandwidth_bps = attrs.get("BANDWIDTH").and_then(|b| b.parse::<u64>().ok());
            let codecs = attrs.get("CODECS").cloned();
            let frame_rate = attrs.get("FRAME-RATE").cloned();

            let id = match height {
                Some(h) => format!("hls-{h}p-{idx}"),
                None => format!("hls-{idx}"),
            };
            idx += 1;

            streams.push(MediaStream {
                id,
                kind: StreamKind::VideoAudio,
                container: "ts".to_string(),
                width,
                height,
                bandwidth_bps,
                codecs,
                frame_rate,
                language: None,
                payload: StreamPayload::Hls {
                    playlist_url: variant_url.to_string(),
                    init_segment_url: None,
                    segments: Vec::new(),
                    is_vod: true,
                },
            });
        }
    }

    if streams.is_empty() {
        return Err(RuviError::Unsupported(
            "HLS master playlist contains no valid variants".to_string(),
        ));
    }

    Ok(MediaSource {
        url: original_url.to_string(),
        resolved_url: base_url.to_string(),
        kind: SourceKind::HlsVod,
        title: derive_hls_title(base_url),
        default_ext: "ts".to_string(),
        streams,
        subtitles,
    })
}
