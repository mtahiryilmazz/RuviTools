use serde_json::{Value, json};
use url::Url;

use crate::domain::media_source::{Chapter, MediaSource, SourceKind, SubtitleTrack};
use crate::domain::stream::{MediaStream, StreamKind, StreamPayload};
use crate::error::{RuviError, RuviResult};
use crate::resolver::ResolveContext;

const ANDROID_USER_AGENT: &str = "com.google.android.youtube/20.10.38 (Linux; U; Android 14) gzip";
const IOS_USER_AGENT: &str =
    "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)";
const WATCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub fn is_youtube_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let lower = host.to_ascii_lowercase();
    lower == "youtu.be"
        || lower == "youtube.com"
        || lower.ends_with(".youtube.com")
        || lower == "youtube-nocookie.com"
        || lower.ends_with(".youtube-nocookie.com")
}

pub fn extract_video_id(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "youtu.be" {
        return url
            .path_segments()
            .and_then(|mut s| s.next())
            .filter(|id| !id.is_empty())
            .map(ToString::to_string);
    }
    for (k, v) in url.query_pairs() {
        if k == "v" && !v.is_empty() {
            return Some(v.into_owned());
        }
    }
    let segs: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    if segs.len() >= 2 && matches!(segs[0], "shorts" | "embed" | "live" | "v") {
        return Some(segs[1].to_string());
    }
    None
}

/// Resolves YouTube media from already-fetched HTML page.
/// 1. Checks if HTML contains embedded `ytInitialPlayerResponse`.
/// 2. If not, queries YouTube's Innertube `/youtubei/v1/player` JSON API
///    with native Android & iOS client contexts.
pub async fn resolve_youtube_from_html(
    ctx: &ResolveContext,
    raw_url: &str,
    final_url: &Url,
    html: &str,
) -> RuviResult<Option<MediaSource>> {
    let parsed_raw = ctx.validate_url(raw_url).ok();
    let is_yt = is_youtube_url(final_url)
        || parsed_raw.as_ref().is_some_and(is_youtube_url)
        || html.contains("ytInitialPlayerResponse")
        || html.contains("youtube.com")
        || html.contains("youtu.be");

    if !is_yt {
        return Ok(None);
    }

    let video_id =
        extract_video_id(final_url).or_else(|| parsed_raw.as_ref().and_then(extract_video_id));

    if let Some(video_id) = video_id {
        let visitor_data = extract_visitor_data(html);
        let innertube_base = Url::parse(&format!(
            "{}://{}{}",
            final_url.scheme(),
            final_url.host_str().unwrap_or("www.youtube.com"),
            final_url
                .port()
                .map(|p| format!(":{p}"))
                .unwrap_or_default()
        ))
        .unwrap_or_else(|_| final_url.clone());

        if let Ok(api_url) = innertube_base.join("/youtubei/v1/player?prettyPrint=false") {
            // Try IOS client first: iOS provides pristine adaptive 1080p/4K and 140 AAC streams
            // that download without bot blocks or HTTP 403.
            // iOS client must NOT receive desktop web visitor_data, which causes Google 403 Forbidden.
            // Fall back to Android client for environments where iOS is restricted.
            for profile in [InnertubeClient::Ios, InnertubeClient::Android] {
                let v_data = match profile {
                    InnertubeClient::Ios => None,
                    InnertubeClient::Android => visitor_data.as_deref(),
                };
                if let Ok(player_json) = query_innertube_player(ctx, &api_url, &video_id, v_data, profile).await {
                    if let Ok(Some(source)) = parse_player_response_json(
                        raw_url,
                        &innertube_base,
                        &player_json,
                        Some(profile.user_agent()),
                    ) {
                        return Ok(Some(source));
                    }
                }
            }
        }
    }

    // Fall back to HTML-embedded ytInitialPlayerResponse (e.g. offline test fixtures)
    if let Ok(Some(native_html_source)) = extract_youtube_player_response(raw_url, final_url, html) {
        return Ok(Some(native_html_source));
    }

    Ok(None)
}

/// Resolves a YouTube URL completely in pure Rust:
/// 1. Fetches watch HTML (supports offline/mock `ytInitialPlayerResponse` fixtures too).
/// 2. Invokes `resolve_youtube_from_html`.
pub async fn resolve_youtube_url(ctx: &ResolveContext, raw_url: &str) -> RuviResult<MediaSource> {
    let parsed = ctx.validate_url(raw_url)?;
    let (html, final_url, _) = fetch_watch_html(ctx, parsed.as_str()).await?;

    if let Some(source) = resolve_youtube_from_html(ctx, raw_url, &final_url, &html).await? {
        return Ok(source);
    }

    Err(RuviError::Unsupported(
        "YouTube Innertube returned no direct playable streams for this video".to_string(),
    ))
}

#[derive(Debug, Clone, Copy)]
enum InnertubeClient {
    Android,
    Ios,
}

impl InnertubeClient {
    fn user_agent(self) -> &'static str {
        match self {
            Self::Android => ANDROID_USER_AGENT,
            Self::Ios => IOS_USER_AGENT,
        }
    }

    fn client_context(self, visitor_data: Option<&str>) -> Value {
        let mut client = match self {
            Self::Android => json!({
                "clientName": "ANDROID",
                "clientVersion": "20.10.38",
                "deviceMake": "Google",
                "deviceModel": "Pixel 8",
                "osName": "Android",
                "osVersion": "14",
                "androidSdkVersion": 34,
                "hl": "en",
                "gl": "US"
            }),
            Self::Ios => json!({
                "clientName": "IOS",
                "clientVersion": "20.10.4",
                "deviceMake": "Apple",
                "deviceModel": "iPhone16,2",
                "osName": "iPhone",
                "osVersion": "18.3.2.22D82",
                "hl": "en",
                "gl": "US"
            }),
        };
        if let Some(vd) = visitor_data
            && let Some(obj) = client.as_object_mut()
        {
            obj.insert("visitorData".to_string(), Value::String(vd.to_string()));
        }
        client
    }
}

async fn fetch_watch_html(ctx: &ResolveContext, url: &str) -> RuviResult<(String, Url, String)> {
    let parsed = ctx.validate_url(url)?;
    let resp = ctx
        .client
        .get(parsed)
        .header(reqwest::header::USER_AGENT, WATCH_USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|err| RuviError::Network(format!("YouTube watch request failed: {err}")))?;
    let final_url = resp.url().clone();
    crate::resolver::validate_url_security(&final_url, ctx.allow_private_net)?;
    if !resp.status().is_success() {
        return Err(RuviError::Network(format!(
            "HTTP {} fetching YouTube watch page {}",
            resp.status().as_u16(),
            final_url
        )));
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp
        .bytes()
        .await
        .map_err(|err| RuviError::Network(format!("failed reading YouTube HTML: {err}")))?;
    if bytes.len() > crate::resolver::MAX_MANIFEST_BYTES {
        return Err(RuviError::Unsupported(
            "YouTube watch page exceeded maximum allowed size".to_string(),
        ));
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), final_url, ct))
}

async fn query_innertube_player(
    ctx: &ResolveContext,
    api_url: &Url,
    video_id: &str,
    visitor_data: Option<&str>,
    profile: InnertubeClient,
) -> RuviResult<Value> {
    let payload = json!({
        "videoId": video_id,
        "context": {
            "client": profile.client_context(visitor_data)
        }
    });

    let mut req = ctx
        .client
        .post(api_url.clone())
        .header(reqwest::header::USER_AGENT, profile.user_agent())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&payload);

    if let Some(vd) = visitor_data {
        req = req.header("X-Goog-Visitor-Id", vd);
    }

    let resp = req
        .send()
        .await
        .map_err(|err| RuviError::Network(format!("YouTube Innertube request failed: {err}")))?;

    if !resp.status().is_success() {
        return Err(RuviError::Network(format!(
            "YouTube Innertube returned HTTP {}",
            resp.status().as_u16()
        )));
    }

    resp.json::<Value>().await.map_err(|err| {
        RuviError::Unsupported(format!("invalid JSON from YouTube Innertube: {err}"))
    })
}

pub fn extract_visitor_data(html: &str) -> Option<String> {
    let marker = "\"VISITOR_DATA\":\"";
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find('"')?;
    let val = &html[start..start + end];
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

pub fn extract_youtube_player_response(
    original_url: &str,
    base_url: &Url,
    html: &str,
) -> RuviResult<Option<MediaSource>> {
    let Some(json_str) = extract_balanced_json_after(html, "ytInitialPlayerResponse") else {
        return Ok(None);
    };

    let root: Value = serde_json::from_str(&json_str).map_err(|err| {
        RuviError::Unsupported(format!("invalid ytInitialPlayerResponse JSON: {err}"))
    })?;

    parse_player_response_json(original_url, base_url, &root, None)
}

pub fn parse_player_response_json(
    original_url: &str,
    base_url: &Url,
    root: &Value,
    client_ua: Option<&str>,
) -> RuviResult<Option<MediaSource>> {
    if let Some(status) = root
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        && !status.eq_ignore_ascii_case("OK")
    {
        let reason = root
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or(status);
        return Err(RuviError::Unsupported(format!(
            "YouTube media not playable ({status}): {reason}"
        )));
    }

    let title = root
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("youtube_video")
        .to_string();

    let mut streams = Vec::new();
    let mut saw_cipher_only = false;

    if let Some(formats) = root
        .pointer("/streamingData/formats")
        .and_then(Value::as_array)
    {
        for fmt in formats {
            if let Some(stream) = parse_player_format(fmt, base_url, true, streams.len()) {
                streams.push(stream);
            } else if fmt.get("signatureCipher").is_some() || fmt.get("cipher").is_some() {
                saw_cipher_only = true;
            }
        }
    }

    if let Some(adaptive) = root
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
    {
        for fmt in adaptive {
            if let Some(mut stream) = parse_player_format(fmt, base_url, false, streams.len()) {
                // Disambiguate if another stream already shares the same itag ID (e.g. multi-language audio dubs)
                if streams.iter().any(|s| s.id == stream.id) {
                    if let Some(ref lang) = stream.language {
                        stream.id = format!("{}-{lang}", stream.id);
                    } else {
                        stream.id = format!("{}-{}", stream.id, streams.len());
                    }
                }
                streams.push(stream);
            }
        }
    }

    if let Some(hls_url) = root
        .pointer("/streamingData/hlsManifestUrl")
        .and_then(Value::as_str)
    {
        let joined = base_url
            .join(hls_url)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| hls_url.to_string());
        streams.push(MediaStream {
            id: "yt-hls".to_string(),
            kind: StreamKind::VideoAudio,
            container: "ts".to_string(),
            width: None,
            height: None,
            bandwidth_bps: None,
            codecs: None,
            frame_rate: None,
            language: None,
            payload: StreamPayload::Hls {
                playlist_url: joined,
                init_segment_url: None,
                segments: Vec::new(),
                is_vod: true,
            },
        });
    }

    if streams.is_empty() {
        if saw_cipher_only {
            return Err(RuviError::Unsupported(
                "YouTube format uses ciphered signature; try Innertube mobile client".to_string(),
            ));
        }
        return Ok(None);
    }

    let mut subtitles = Vec::new();
    if let Some(captions) = root
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
    {
        for cap in captions {
            if let Some(sub_url) = cap.get("baseUrl").and_then(Value::as_str) {
                let joined = base_url
                    .join(sub_url)
                    .map(|u| u.to_string())
                    .unwrap_or_else(|_| sub_url.to_string());
                let lang = cap
                    .get("languageCode")
                    .and_then(Value::as_str)
                    .unwrap_or("und")
                    .to_string();
                let name = cap
                    .pointer("/name/simpleText")
                    .or_else(|| cap.pointer("/name/runs/0/text"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let is_auto = cap
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|k| k.eq_ignore_ascii_case("asr"))
                    || cap
                        .get("vssId")
                        .and_then(Value::as_str)
                        .is_some_and(|v| v.starts_with("a."));
                subtitles.push(SubtitleTrack {
                    language: lang,
                    name,
                    url: joined,
                    format: "vtt".to_string(),
                    is_auto,
                });
            }
        }
    }

    let chapters = parse_youtube_chapters(root);

    let default_ext = streams
        .iter()
        .find(|s| s.kind == StreamKind::VideoAudio)
        .or_else(|| streams.first())
        .map(|s| s.container.clone())
        .unwrap_or_else(|| "mp4".to_string());

    Ok(Some(MediaSource {
        url: original_url.to_string(),
        resolved_url: base_url.to_string(),
        kind: SourceKind::PlatformHtml,
        title,
        default_ext,
        streams,
        subtitles,
        chapters,
        user_agent_override: client_ua.map(ToString::to_string),
    }))
}

fn parse_player_format(
    fmt: &Value,
    base_url: &Url,
    is_muxed: bool,
    idx: usize,
) -> Option<MediaStream> {
    let raw_stream_url = fmt.get("url").and_then(Value::as_str)?;
    let joined = base_url
        .join(raw_stream_url)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| raw_stream_url.to_string());
    let mime = fmt
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("video/mp4");

    let is_audio = mime.starts_with("audio/");
    let container = if mime.contains("webm") {
        "webm"
    } else if is_audio && mime.contains("mp4") {
        "m4a"
    } else {
        "mp4"
    };

    let kind = if is_muxed {
        StreamKind::VideoAudio
    } else if is_audio {
        StreamKind::Audio
    } else {
        StreamKind::Video
    };

    let codecs = mime
        .split("codecs=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .map(ToString::to_string);
    let total_bytes = fmt
        .get("contentLength")
        .and_then(Value::as_u64)
        .or_else(|| {
            fmt.get("contentLength")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
        });
    let bandwidth_bps = fmt
        .get("bitrate")
        .and_then(Value::as_u64)
        .or_else(|| {
            fmt.get("bitrate")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
        })
        .or_else(|| fmt.get("averageBitrate").and_then(Value::as_u64))
        .or_else(|| {
            fmt.get("averageBitrate")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
        });
    let itag = fmt
        .get("itag")
        .and_then(Value::as_u64)
        .map(|id| format!("itag-{id}"))
        .unwrap_or_else(|| format!("yt-{idx}"));

    Some(MediaStream {
        id: itag,
        kind,
        container: container.to_string(),
        width: fmt.get("width").and_then(Value::as_u64).map(|v| v as u32),
        height: fmt.get("height").and_then(Value::as_u64).map(|v| v as u32),
        bandwidth_bps,
        codecs,
        frame_rate: fmt
            .get("fps")
            .and_then(Value::as_u64)
            .map(|f| f.to_string()),
        language: extract_audio_language(fmt),
        payload: StreamPayload::DirectHttp {
            url: joined,
            total_bytes,
            accept_ranges: total_bytes.is_some(),
            etag: None,
            last_modified: None,
        },
    })
}

pub fn extract_balanced_json_after(html: &str, marker: &str) -> Option<String> {
    let idx = html.find(marker)?;
    let after = &html[idx + marker.len()..];
    extract_balanced_json_from_slice(after)
}

pub fn extract_balanced_json_from_slice(slice: &str) -> Option<String> {
    let brace_rel = slice.find('{')?;
    let json_slice = &slice[brace_rel..];

    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in json_slice.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(json_slice[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_audio_language(fmt: &Value) -> Option<String> {
    if let Some(id) = fmt.pointer("/audioTrack/id").and_then(Value::as_str) {
        let lang = id.split('.').next().unwrap_or(id).trim();
        if !lang.is_empty() {
            return Some(lang.to_string());
        }
    }
    if let Some(code) = fmt.get("languageCode").and_then(Value::as_str) {
        let lang = code.trim();
        if !lang.is_empty() {
            return Some(lang.to_string());
        }
    }
    if let Some(display) = fmt
        .pointer("/audioTrack/displayName")
        .and_then(Value::as_str)
    {
        let lang = display.split_whitespace().next().unwrap_or(display).trim();
        if !lang.is_empty() {
            return Some(lang.to_ascii_lowercase());
        }
    }
    None
}

pub fn parse_youtube_chapters(root: &Value) -> Vec<Chapter> {
    let total_dur = root
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
        })
        .unwrap_or(0.0);

    let mut raw: Vec<(String, f64)> = Vec::new();

    // 1. Check markersMap (top-level or nested under playerOverlays)
    let marker_arrays = [
        root.get("markersMap").and_then(Value::as_array),
        root.pointer(
            "/playerOverlays/playerOverlayRenderer/decoratedPlayerBarRenderer/decoratedPlayerBarRenderer/playerBar/multiMarkersPlayerBarRenderer/markersMap",
        )
        .and_then(Value::as_array),
    ];

    for arr in marker_arrays.into_iter().flatten() {
        for entry in arr {
            let key = entry.get("key").and_then(Value::as_str).unwrap_or("");
            if !matches!(
                key,
                "DESCRIPTION_CHAPTERS" | "AUTO_CHAPTERS" | "MACRO_MARKERS"
            ) && !key.is_empty()
            {
                continue;
            }
            if let Some(chaps) = entry.pointer("/value/chapters").and_then(Value::as_array) {
                for ch in chaps {
                    let renderer = ch.get("chapterRenderer").unwrap_or(ch);
                    let title = renderer
                        .pointer("/title/simpleText")
                        .or_else(|| renderer.pointer("/title/runs/0/text"))
                        .and_then(Value::as_str)
                        .unwrap_or("Chapter")
                        .trim()
                        .to_string();
                    let start_ms = renderer
                        .get("timeRangeStartMillis")
                        .and_then(|v| {
                            v.as_f64()
                                .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
                        })
                        .unwrap_or(0.0);
                    let start_s = (start_ms / 1000.0).max(0.0);
                    if total_dur <= 0.0 || start_s < total_dur {
                        raw.push((title, start_s));
                    }
                }
            }
            if !raw.is_empty() {
                break;
            }
        }
        if !raw.is_empty() {
            break;
        }
    }

    // 2. Fallback: parse timestamp lines from shortDescription
    if raw.is_empty()
        && let Some(desc) = root
            .pointer("/videoDetails/shortDescription")
            .and_then(Value::as_str)
    {
        for line in desc.lines() {
            if let Some((ts_s, title)) = parse_description_chapter_line(line)
                && (total_dur <= 0.0 || ts_s < total_dur)
            {
                raw.push((title, ts_s));
            }
        }
    }

    if raw.is_empty() {
        return Vec::new();
    }

    raw.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    raw.dedup_by(|a, b| (a.1 - b.1).abs() < 0.001);

    let mut chapters = Vec::with_capacity(raw.len());
    for i in 0..raw.len() {
        let (ref title, start_s) = raw[i];
        let end_s = if i + 1 < raw.len() {
            raw[i + 1].1.max(start_s)
        } else if total_dur > start_s {
            total_dur
        } else {
            start_s + 60.0
        };
        chapters.push(Chapter {
            title: title.clone(),
            start_seconds: start_s,
            end_seconds: end_s,
        });
    }
    chapters
}

fn parse_description_chapter_line(line: &str) -> Option<(f64, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next()?.trim_matches(['(', ')', '[', ']', '-']);
    let rest = parts
        .next()?
        .trim_start_matches(['-', '–', '—', ':', ' '])
        .trim();
    if rest.is_empty() || !first.contains(':') {
        return None;
    }
    let secs = parse_timestamp_seconds(first)?;
    Some((secs, rest.to_string()))
}

pub fn parse_timestamp_seconds(input: &str) -> Option<f64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    let total = match parts.len() {
        1 => parts[0].parse::<f64>().ok()?,
        2 => {
            let m = parts[0].parse::<f64>().ok()?;
            let sec = parts[1].parse::<f64>().ok()?;
            m * 60.0 + sec
        }
        3 => {
            let h = parts[0].parse::<f64>().ok()?;
            let m = parts[1].parse::<f64>().ok()?;
            let sec = parts[2].parse::<f64>().ok()?;
            h * 3600.0 + m * 60.0 + sec
        }
        _ => return None,
    };
    if total.is_finite() && total >= 0.0 {
        Some(total)
    } else {
        None
    }
}

/// Converts subtitle text (YouTube XML `<transcript>` / `<timedtext>` or WebVTT) into the target format (`vtt`, `srt`, `sbv`).
pub fn convert_subtitle_payload(body: &str, target_format: &str) -> String {
    let fmt = target_format.trim().to_ascii_lowercase();
    let trimmed = body.trim();
    let is_xml = trimmed.starts_with("<?xml")
        || trimmed.starts_with("<transcript")
        || trimmed.starts_with("<timedtext");

    if !is_xml {
        if fmt == "vtt" && trimmed.starts_with("WEBVTT") {
            return body.to_string();
        }
        if fmt == "srt" && !trimmed.starts_with("WEBVTT") && trimmed.contains("-->") {
            return body.to_string();
        }
        // Convert simple WebVTT cues to SRT or SBV if requested
        if trimmed.starts_with("WEBVTT") && (fmt == "srt" || fmt == "sbv") {
            let cues = parse_vtt_cues(trimmed);
            if !cues.is_empty() {
                return render_cues(&cues, &fmt);
            }
        }
        return body.to_string();
    }

    let cues = parse_xml_timedtext_cues(trimmed);
    render_cues(&cues, &fmt)
}

fn parse_xml_timedtext_cues(xml: &str) -> Vec<(u64, u64, String)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut cues = Vec::new();
    let mut current_range: Option<(u64, u64)> = None;
    let mut current_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                if name.as_ref() == b"text" || name.as_ref() == b"p" {
                    let mut start_ms = None;
                    let mut dur_ms = None;
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let val = String::from_utf8_lossy(&attr.value);
                        if key == b"start" {
                            start_ms = val.parse::<f64>().ok().map(|s| (s * 1000.0).round() as u64);
                        } else if key == b"dur" {
                            dur_ms = val.parse::<f64>().ok().map(|d| (d * 1000.0).round() as u64);
                        } else if key == b"t" {
                            start_ms = val.parse::<u64>().ok();
                        } else if key == b"d" {
                            dur_ms = val.parse::<u64>().ok();
                        }
                    }
                    if let Some(s) = start_ms {
                        let d = dur_ms.unwrap_or(2500);
                        current_range = Some((s, s + d));
                        current_text.clear();
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if current_range.is_some() {
                    let raw = String::from_utf8_lossy(t.as_ref());
                    current_text.push_str(&unescape_xml_entities(&raw));
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                if (name.as_ref() == b"text" || name.as_ref() == b"p")
                    && let Some((s, end)) = current_range.take()
                {
                    let clean = current_text.trim().to_string();
                    if !clean.is_empty() {
                        cues.push((s, end, clean));
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    cues
}

fn parse_vtt_cues(vtt: &str) -> Vec<(u64, u64, String)> {
    let mut cues = Vec::new();
    for block in vtt.split("\n\n") {
        let mut lines = block.lines();
        let Some(first) = lines.next() else {
            continue;
        };
        let ts_line = if first.contains("-->") {
            first
        } else {
            let Some(second) = lines.next() else {
                continue;
            };
            if !second.contains("-->") {
                continue;
            }
            second
        };
        let Some((a, b)) = ts_line.split_once("-->") else {
            continue;
        };
        let Some(start_s) = parse_timestamp_seconds(a.trim()) else {
            continue;
        };
        let end_token = b.split_whitespace().next().unwrap_or("");
        let Some(end_s) = parse_timestamp_seconds(end_token) else {
            continue;
        };
        let text: String = lines.collect::<Vec<_>>().join("\n");
        if !text.trim().is_empty() {
            cues.push((
                (start_s * 1000.0).round() as u64,
                (end_s * 1000.0).round() as u64,
                text,
            ));
        }
    }
    cues
}

fn render_cues(cues: &[(u64, u64, String)], fmt: &str) -> String {
    match fmt {
        "srt" => {
            let mut out = String::new();
            for (idx, (start_ms, end_ms, text)) in cues.iter().enumerate() {
                out.push_str(&format!(
                    "{}\n{} --> {}\n{}\n\n",
                    idx + 1,
                    fmt_ts(*start_ms, ','),
                    fmt_ts(*end_ms, ','),
                    text
                ));
            }
            out
        }
        "sbv" => {
            let mut out = String::new();
            for (start_ms, end_ms, text) in cues {
                out.push_str(&format!(
                    "{},{}\n{}\n\n",
                    fmt_ts(*start_ms, '.'),
                    fmt_ts(*end_ms, '.'),
                    text
                ));
            }
            out
        }
        _ => {
            let mut out = String::from("WEBVTT\n\n");
            for (start_ms, end_ms, text) in cues {
                out.push_str(&format!(
                    "{} --> {}\n{}\n\n",
                    fmt_ts(*start_ms, '.'),
                    fmt_ts(*end_ms, '.'),
                    text
                ));
            }
            out
        }
    }
}

fn fmt_ts(ms: u64, sep: char) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{sep}{millis:03}")
}

fn unescape_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}
