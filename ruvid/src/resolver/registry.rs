use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use url::Url;

use crate::domain::media_manifest::redact_sensitive_url;
use crate::domain::media_source::{MediaSource, SourceKind};
use crate::domain::stream::StreamPayload;
use crate::error::{RuviError, RuviResult};
use crate::resolver::dash::resolve_dash_from_xml;
use crate::resolver::direct_http::{probe_http_resource, resolve_direct_http_from_probe};
use crate::resolver::hls::resolve_hls;
use crate::resolver::platform::youtube::{
    extract_balanced_json_after, extract_balanced_json_from_slice, is_youtube_url,
    resolve_youtube_from_html, resolve_youtube_url,
};
use crate::resolver::platform::{
    HtmlMetadataPlatformResolver, PlatformResolver, extract_title_from_html, with_html_title,
};
use crate::resolver::{ResolveContext, validate_url_security};

pub struct ResolverRegistry;

impl ResolverRegistry {
    pub async fn resolve(ctx: &ResolveContext, raw_url: &str) -> RuviResult<MediaSource> {
        let source = Self::resolve_inner(ctx, raw_url, 0).await?;
        validate_media_source_urls(&source, ctx.allow_private_net)?;
        Ok(source)
    }

    #[allow(clippy::too_many_lines)]
    fn resolve_inner<'a>(
        ctx: &'a ResolveContext,
        raw_url: &'a str,
        depth: u8,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = RuviResult<MediaSource>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > 2 {
                return Err(RuviError::Unsupported(
                    "resolver redirect/platform recursion depth exceeded".to_string(),
                ));
            }

            let parsed = ctx.validate_url(raw_url)?;
            if is_youtube_url(&parsed) {
                return resolve_youtube_url(ctx, raw_url).await;
            }
            let url_path = parsed.path().to_ascii_lowercase();

            if url_path.ends_with(".m3u8") {
                let (body, final_url, _) = ctx.fetch_text_limited(raw_url).await?;
                return resolve_hls(ctx, raw_url, &final_url, &body, depth).await;
            }

            if url_path.ends_with(".mpd") {
                let (body, final_url, _) = ctx.fetch_text_limited(raw_url).await?;
                return resolve_dash_from_xml(raw_url, &final_url, &body);
            }

            let probe = probe_http_resource(ctx, raw_url).await?;
            let ct = probe.content_type.as_deref().unwrap_or("");
            let final_path = probe.final_url.path().to_ascii_lowercase();

            if final_path.ends_with(".m3u8")
                || ct.contains("mpegurl")
                || ct.contains("vnd.apple.mpegurl")
            {
                let (body, final_url, _) = ctx.fetch_text_limited(probe.final_url.as_str()).await?;
                return resolve_hls(ctx, raw_url, &final_url, &body, depth).await;
            }

            if final_path.ends_with(".mpd") || ct.contains("dash+xml") {
                let (body, final_url, _) = ctx.fetch_text_limited(probe.final_url.as_str()).await?;
                return resolve_dash_from_xml(raw_url, &final_url, &body);
            }

            let html_resolver = HtmlMetadataPlatformResolver;
            if html_resolver.can_handle(&probe.final_url, ct) {
                let (html, final_url, _) = ctx.fetch_text_limited(probe.final_url.as_str()).await?;
                if let Some(yt_source) =
                    resolve_youtube_from_html(ctx, raw_url, &final_url, &html).await?
                {
                    return Ok(yt_source);
                }
                let page_title = extract_title_from_html(&html);
                if let Some(target) = html_resolver
                    .resolve(ctx, raw_url, &final_url, &html)
                    .await?
                {
                    let mut source = Self::resolve_inner(ctx, &target, depth + 1).await?;
                    if source.kind == SourceKind::DirectHttp {
                        source.kind = SourceKind::PlatformHtml;
                    }
                    return Ok(with_html_title(source, page_title));
                }
                return Err(RuviError::Unsupported(format!(
                    "no playable media link found in HTML page {}",
                    redact_sensitive_url(final_url.as_str())
                )));
            }

            Ok(resolve_direct_http_from_probe(raw_url, probe))
        })
    }
}

fn validate_media_source_urls(source: &MediaSource, allow_private_net: bool) -> RuviResult<()> {
    let check_url = |raw: &str| -> RuviResult<()> {
        let parsed = Url::parse(raw).map_err(|err| {
            RuviError::Unsupported(format!(
                "invalid resolved URL '{}': {err}",
                redact_sensitive_url(raw)
            ))
        })?;
        validate_url_security(&parsed, allow_private_net)
    };

    for stream in &source.streams {
        match &stream.payload {
            StreamPayload::DirectHttp { url, .. } => check_url(url)?,
            StreamPayload::Hls {
                playlist_url,
                init_segment_url,
                segments,
                ..
            } => {
                check_url(playlist_url)?;
                if let Some(init) = init_segment_url {
                    check_url(init)?;
                }
                for seg in segments {
                    check_url(&seg.url)?;
                }
            }
            StreamPayload::Dash {
                mpd_url,
                init_segment_url,
                segments,
            } => {
                check_url(mpd_url)?;
                if let Some(init) = init_segment_url {
                    check_url(init)?;
                }
                for seg in segments {
                    check_url(&seg.url)?;
                }
            }
        }
    }
    for sub in &source.subtitles {
        check_url(&sub.url)?;
    }
    Ok(())
}

pub fn parse_item_selection(spec: &str, total: usize) -> RuviResult<Vec<usize>> {
    if spec.trim().is_empty() {
        return Ok((1..=total).collect());
    }
    let mut selected = std::collections::BTreeSet::new();
    for part in spec.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Some((a_str, b_str)) = p.split_once('-') {
            let a = a_str
                .trim()
                .parse::<usize>()
                .map_err(|_| RuviError::InvalidArgument(format!("invalid item range '{p}'")))?;
            let b = b_str
                .trim()
                .parse::<usize>()
                .map_err(|_| RuviError::InvalidArgument(format!("invalid item range '{p}'")))?;
            if a == 0 || b < a {
                return Err(RuviError::InvalidArgument(format!(
                    "invalid item range '{p}'"
                )));
            }
            for idx in a..=b.min(total) {
                selected.insert(idx);
            }
        } else {
            let idx = p
                .parse::<usize>()
                .map_err(|_| RuviError::InvalidArgument(format!("invalid item index '{p}'")))?;
            if idx == 0 {
                return Err(RuviError::InvalidArgument(
                    "playlist item indices are 1-based".to_string(),
                ));
            }
            if idx <= total {
                selected.insert(idx);
            }
        }
    }
    Ok(selected.into_iter().collect())
}

#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    pub index: usize,
    pub title: Option<String>,
    pub url: String,
}

pub async fn fetch_playlist_entries(
    ctx: &ResolveContext,
    playlist_url: &str,
) -> RuviResult<Vec<PlaylistEntry>> {
    let parsed = ctx.validate_url(playlist_url)?;
    if is_youtube_url(&parsed) {
        let resp = ctx
            .client
            .get(parsed.clone())
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            )
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .send()
            .await
            .map_err(|err| RuviError::Network(format!("YouTube playlist request failed: {err}")))?;
        let final_url = resp.url().clone();
        validate_url_security(&final_url, ctx.allow_private_net)?;
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await.map_err(|err| {
            RuviError::Network(format!("failed reading YouTube playlist HTML: {err}"))
        })?;
        let body = String::from_utf8_lossy(&bytes).into_owned();
        return parse_playlist_body(&body, &final_url, &ct);
    }
    let (body, base_url, content_type) = ctx.fetch_text_limited(playlist_url).await?;
    parse_playlist_body(&body, &base_url, &content_type)
}

pub fn parse_playlist_body(
    body: &str,
    base_url: &Url,
    content_type: &str,
) -> RuviResult<Vec<PlaylistEntry>> {
    let trimmed = body.trim();

    // 1. RSS / Atom podcast XML feeds
    let is_xml = content_type.contains("xml")
        || trimmed.starts_with("<?xml")
        || trimmed.starts_with("<rss")
        || trimmed.starts_with("<feed")
        || trimmed.contains("<rss")
        || trimmed.contains("<feed")
        || (trimmed.contains("<channel") && trimmed.contains("<item"));

    if is_xml {
        if let Ok(entries) = parse_xml_podcast_feed(body, base_url) {
            return Ok(entries);
        }
    }

    // 2. YouTube playlist HTML or embedded ytInitialData
    let is_yt = is_youtube_url(base_url)
        || trimmed.contains("ytInitialData")
        || trimmed.contains("playlistVideoRenderer")
        || trimmed.contains("playlistPanelVideoRenderer");

    if is_yt {
        if let Ok(entries) = parse_youtube_playlist_data(body, base_url) {
            return Ok(entries);
        }
    }

    // 3. JSON catalog
    if content_type.contains("json") || trimmed.starts_with('[') || trimmed.starts_with('{') {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum JsonCatalog {
            Array(Vec<JsonEntry>),
            Wrapped { items: Vec<JsonEntry> },
        }
        #[derive(serde::Deserialize)]
        struct JsonEntry {
            url: String,
            title: Option<String>,
        }
        let parsed: JsonCatalog = serde_json::from_str(trimmed).map_err(|err| {
            RuviError::Unsupported(format!("invalid JSON playlist catalog: {err}"))
        })?;
        let raw_items = match parsed {
            JsonCatalog::Array(v) => v,
            JsonCatalog::Wrapped { items } => items,
        };
        let mut entries = Vec::with_capacity(raw_items.len());
        for (i, item) in raw_items.into_iter().enumerate() {
            let joined = base_url
                .join(&item.url)
                .map_err(|err| RuviError::Unsupported(err.to_string()))?;
            entries.push(PlaylistEntry {
                index: i + 1,
                title: item.title,
                url: joined.to_string(),
            });
        }
        return Ok(entries);
    }

    // 4. M3U / plain line-based playlist
    let mut entries = Vec::new();
    let mut pending_title: Option<String> = None;
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            if let Some((_, title)) = info.split_once(',') {
                pending_title = Some(title.trim().to_string());
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let joined = base_url
            .join(line)
            .map_err(|err| RuviError::Unsupported(err.to_string()))?;
        entries.push(PlaylistEntry {
            index: entries.len() + 1,
            title: pending_title.take(),
            url: joined.to_string(),
        });
    }

    if entries.is_empty() {
        return Err(RuviError::Unsupported(
            "playlist contains no entries".to_string(),
        ));
    }
    Ok(entries)
}

pub fn parse_xml_podcast_feed(body: &str, base_url: &Url) -> RuviResult<Vec<PlaylistEntry>> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut in_item = false;
    let mut in_title = false;
    let mut current_title = String::new();
    let mut current_url: Option<String> = None;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.name().local_name();
                if local.as_ref().eq_ignore_ascii_case(b"item")
                    || local.as_ref().eq_ignore_ascii_case(b"entry")
                {
                    in_item = true;
                    current_title.clear();
                    current_url = None;
                } else if in_item && local.as_ref().eq_ignore_ascii_case(b"title") {
                    in_title = true;
                    current_title.clear();
                } else if in_item {
                    if let Some(u) = extract_enclosure_url(e) {
                        current_url = Some(u);
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                if in_item {
                    if let Some(u) = extract_enclosure_url(e) {
                        current_url = Some(u);
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_title {
                    if let Ok(txt) = e.unescape() {
                        current_title.push_str(&txt);
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                if in_title {
                    if let Ok(txt) = std::str::from_utf8(e.as_ref()) {
                        current_title.push_str(txt);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.name().local_name();
                if in_item && local.as_ref().eq_ignore_ascii_case(b"title") {
                    in_title = false;
                } else if local.as_ref().eq_ignore_ascii_case(b"item")
                    || local.as_ref().eq_ignore_ascii_case(b"entry")
                {
                    in_item = false;
                    if let Some(raw_u) = current_url.take() {
                        let joined = base_url
                            .join(&raw_u)
                            .map_err(|err| RuviError::Unsupported(err.to_string()))?;
                        let title = if current_title.trim().is_empty() {
                            None
                        } else {
                            Some(current_title.trim().to_string())
                        };
                        entries.push(PlaylistEntry {
                            index: entries.len() + 1,
                            title,
                            url: joined.to_string(),
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(RuviError::Unsupported(format!(
                    "XML podcast parsing error: {e}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    if entries.is_empty() {
        return Err(RuviError::Unsupported(
            "XML feed contained no media enclosure entries".to_string(),
        ));
    }

    Ok(entries)
}

fn extract_enclosure_url(e: &BytesStart) -> Option<String> {
    let local = e.name().local_name();
    if local.as_ref().eq_ignore_ascii_case(b"enclosure") {
        for attr in e.attributes().flatten() {
            let k = attr.key.local_name();
            if k.as_ref().eq_ignore_ascii_case(b"url") || k.as_ref().eq_ignore_ascii_case(b"href") {
                if let Ok(val) = std::str::from_utf8(&attr.value) {
                    if !val.trim().is_empty() {
                        return Some(val.trim().to_string());
                    }
                }
            }
        }
    } else if local.as_ref().eq_ignore_ascii_case(b"link") {
        let mut is_enclosure = false;
        let mut href = None;
        for attr in e.attributes().flatten() {
            let k = attr.key.local_name();
            if k.as_ref().eq_ignore_ascii_case(b"rel") {
                if let Ok(val) = std::str::from_utf8(&attr.value) {
                    if val.eq_ignore_ascii_case("enclosure") {
                        is_enclosure = true;
                    }
                }
            } else if k.as_ref().eq_ignore_ascii_case(b"href") {
                if let Ok(val) = std::str::from_utf8(&attr.value) {
                    if !val.trim().is_empty() {
                        href = Some(val.trim().to_string());
                    }
                }
            }
        }
        if is_enclosure {
            return href;
        }
    }
    None
}

pub fn parse_youtube_playlist_data(body: &str, _base_url: &Url) -> RuviResult<Vec<PlaylistEntry>> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Try ytInitialData JSON
    if let Some(json_str) = extract_balanced_json_after(body, "ytInitialData") {
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(&json_str) {
            collect_yt_playlist_renderers(&root, &mut entries, &mut seen);
        }
    }

    // 2. Fallback to searching for playlistVideoRenderer markers
    if entries.is_empty() {
        let marker = "\"playlistVideoRenderer\":{";
        let mut pos = 0;
        while let Some(idx) = body[pos..].find(marker) {
            let abs_idx = pos + idx + marker.len() - 1;
            if let Some(slice) = extract_balanced_json_from_slice(&body[abs_idx..]) {
                if let Ok(item) = serde_json::from_str::<serde_json::Value>(&slice) {
                    if let Some(vid) = item.get("videoId").and_then(serde_json::Value::as_str) {
                        if seen.insert(vid.to_string()) {
                            let title = item
                                .pointer("/title/runs/0/text")
                                .and_then(serde_json::Value::as_str)
                                .or_else(|| {
                                    item.pointer("/title/simpleText")
                                        .and_then(serde_json::Value::as_str)
                                })
                                .map(ToString::to_string);
                            entries.push(PlaylistEntry {
                                index: entries.len() + 1,
                                title,
                                url: format!("https://www.youtube.com/watch?v={vid}"),
                            });
                        }
                    }
                }
            }
            pos = abs_idx + 1;
        }
    }

    if entries.is_empty() {
        return Err(RuviError::Unsupported(
            "no playlist video items found in YouTube response".to_string(),
        ));
    }

    Ok(entries)
}

fn collect_yt_playlist_renderers(
    val: &serde_json::Value,
    entries: &mut Vec<PlaylistEntry>,
    seen: &mut std::collections::HashSet<String>,
) {
    match val {
        serde_json::Value::Object(map) => {
            if let Some(item) = map
                .get("playlistVideoRenderer")
                .or_else(|| map.get("playlistPanelVideoRenderer"))
            {
                if let Some(vid) = item.get("videoId").and_then(serde_json::Value::as_str) {
                    if seen.insert(vid.to_string()) {
                        let title = item
                            .pointer("/title/runs/0/text")
                            .and_then(serde_json::Value::as_str)
                            .or_else(|| {
                                item.pointer("/title/simpleText")
                                    .and_then(serde_json::Value::as_str)
                            })
                            .map(ToString::to_string);
                        entries.push(PlaylistEntry {
                            index: entries.len() + 1,
                            title,
                            url: format!("https://www.youtube.com/watch?v={vid}"),
                        });
                    }
                }
            }
            for v in map.values() {
                collect_yt_playlist_renderers(v, entries, seen);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_yt_playlist_renderers(v, entries, seen);
            }
        }
        _ => {}
    }
}
