use serde_json::{Value, json};
use url::Url;

use crate::domain::media_manifest::redact_sensitive_url;
use crate::domain::media_source::{MediaSource, SourceKind, SubtitleTrack};
use crate::domain::stream::{MediaStream, StreamKind, StreamPayload};
use crate::downloader::retry::map_reqwest_error;
use crate::error::{RuviError, RuviResult};
use crate::resolver::{
    MAX_MANIFEST_BYTES, ResolveContext, read_response_bytes_limited, read_response_text_limited,
    validate_url_security,
};

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

pub fn looks_like_youtube_watch_html(url: &Url, html: &str) -> bool {
    if html.contains("ytInitialPlayerResponse") {
        return true;
    }
    let has_vid = extract_video_id(url).is_some() || extract_video_id_from_html(html).is_some();
    has_vid
        && (html.contains("VISITOR_DATA")
            || html.contains("INNERTUBE_API_KEY")
            || html.contains("/youtubei/v1/player"))
}

pub fn extract_video_id_from_html(html: &str) -> Option<String> {
    let marker = "\"videoId\":\"";
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find('"')?;
    let val = html[start..start + end].trim();
    if val.is_empty() || val.len() > 64 {
        None
    } else {
        Some(val.to_string())
    }
}

/// Resolves a YouTube URL completely in pure Rust:
/// 1. Fetches watch HTML (supports offline/mock `ytInitialPlayerResponse` fixtures too).
/// 2. Extracts `VISITOR_DATA` and queries YouTube's Innertube `/youtubei/v1/player` JSON API
///    with native Android & iOS client contexts (zero external `yt-dlp` or `deno` processes).
pub async fn resolve_youtube_url(ctx: &ResolveContext, raw_url: &str) -> RuviResult<MediaSource> {
    let parsed = ctx.validate_url(raw_url)?;
    let (html, final_url, _) = fetch_watch_html(ctx, parsed.as_str()).await?;
    resolve_youtube_from_html(ctx, raw_url, &final_url, &html)
        .await?
        .ok_or_else(|| {
            RuviError::Unsupported(
                "YouTube Innertube returned no direct playable streams for this video".to_string(),
            )
        })
}

pub async fn resolve_youtube_from_html(
    ctx: &ResolveContext,
    raw_url: &str,
    final_url: &Url,
    html: &str,
) -> RuviResult<Option<MediaSource>> {
    let yt_hint = is_youtube_url(final_url) || looks_like_youtube_watch_html(final_url, html);

    // Check if the HTML itself already carries direct playable URLs (e.g. test fixtures)
    let html_attempt = extract_youtube_player_response(raw_url, final_url, html);
    match html_attempt {
        Ok(Some(native_html_source)) => return Ok(Some(native_html_source)),
        Ok(None) if !yt_hint => return Ok(None),
        Err(err) if !yt_hint => return Err(err),
        Ok(None) | Err(_) => {}
    }

    let parsed_raw = Url::parse(raw_url).ok();
    let video_id = extract_video_id(final_url)
        .or_else(|| parsed_raw.as_ref().and_then(extract_video_id))
        .or_else(|| extract_video_id_from_html(html))
        .ok_or_else(|| {
            RuviError::Unsupported(format!(
                "could not extract YouTube video ID from URL '{}'",
                redact_sensitive_url(raw_url)
            ))
        })?;

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

    let api_url = innertube_base
        .join("/youtubei/v1/player?prettyPrint=false")
        .map_err(|err| RuviError::Unsupported(err.to_string()))?;

    // Try ANDROID client first (provides direct muxed progressive streams + adaptive streams),
    // then fall back to IOS client (provides HLS + adaptive streams).
    let mut last_err = None;
    for profile in [InnertubeClient::Android, InnertubeClient::Ios] {
        match query_innertube_player(ctx, &api_url, &video_id, visitor_data.as_deref(), profile)
            .await
        {
            Ok(player_json) => match parse_player_response_json(
                raw_url,
                &innertube_base,
                &player_json,
                Some(profile.user_agent()),
            ) {
                Ok(Some(source)) => return Ok(Some(source)),
                Ok(None) => {}
                Err(err) => last_err = Some(err),
            },
            Err(err) => {
                last_err = Some(err);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        RuviError::Unsupported(
            "YouTube Innertube returned no direct playable streams for this video".to_string(),
        )
    }))
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
        .map_err(map_reqwest_error)?;
    let final_url = resp.url().clone();
    validate_url_security(&final_url, ctx.allow_private_net)?;
    if !resp.status().is_success() {
        return Err(RuviError::Network(format!(
            "HTTP {} fetching YouTube watch page {}",
            resp.status().as_u16(),
            redact_sensitive_url(final_url.as_str())
        )));
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let html = read_response_text_limited(resp, MAX_MANIFEST_BYTES).await?;
    Ok((html, final_url, ct))
}

async fn query_innertube_player(
    ctx: &ResolveContext,
    api_url: &Url,
    video_id: &str,
    visitor_data: Option<&str>,
    profile: InnertubeClient,
) -> RuviResult<Value> {
    validate_url_security(api_url, ctx.allow_private_net)?;
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

    let resp = req.send().await.map_err(map_reqwest_error)?;
    let final_url = resp.url().clone();
    validate_url_security(&final_url, ctx.allow_private_net)?;

    if !resp.status().is_success() {
        return Err(RuviError::Network(format!(
            "YouTube Innertube returned HTTP {}",
            resp.status().as_u16()
        )));
    }

    let bytes = read_response_bytes_limited(resp, MAX_MANIFEST_BYTES).await?;
    serde_json::from_slice::<Value>(&bytes).map_err(|err| {
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
            if let Some(stream) = parse_player_format(fmt, base_url, false, streams.len()) {
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
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                subtitles.push(SubtitleTrack {
                    language: lang,
                    name,
                    url: joined,
                    format: "vtt".to_string(),
                });
            }
        }
    }

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
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok());
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
        bandwidth_bps: fmt.get("bitrate").and_then(Value::as_u64),
        codecs,
        frame_rate: fmt
            .get("fps")
            .and_then(Value::as_u64)
            .map(|f| f.to_string()),
        language: None,
        payload: StreamPayload::DirectHttp {
            url: joined,
            total_bytes,
            accept_ranges: total_bytes.is_some(),
            etag: None,
            last_modified: None,
        },
    })
}

fn extract_balanced_json_after(html: &str, marker: &str) -> Option<String> {
    let mut search_from = 0usize;
    while let Some(rel_idx) = html[search_from..].find(marker) {
        let marker_end = search_from + rel_idx + marker.len();
        let after = &html[marker_end..];
        let brace_rel = after.find('{')?;
        let between = &after[..brace_rel];
        if !between.contains('=') || between.len() > 32 {
            search_from = marker_end;
            continue;
        }
        let json_slice = &after[brace_rel..];

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
        search_from = marker_end;
    }
    None
}
