use url::Url;

use crate::domain::media_source::{MediaSource, SourceKind};
use crate::error::{RuviError, RuviResult};
use crate::resolver::ResolveContext;
use crate::resolver::dash::resolve_dash_from_xml;
use crate::resolver::direct_http::{probe_http_resource, resolve_direct_http_from_probe};
use crate::resolver::hls::resolve_hls;
use crate::resolver::platform::{
    HtmlMetadataPlatformResolver, PlatformResolver, extract_title_from_html, with_html_title,
};

pub struct ResolverRegistry;

impl ResolverRegistry {
    pub async fn resolve(ctx: &ResolveContext, raw_url: &str) -> RuviResult<MediaSource> {
        Self::resolve_inner(ctx, raw_url, 0).await
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
                    final_url
                )));
            }

            Ok(resolve_direct_http_from_probe(raw_url, probe))
        })
    }
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
    let (body, base_url, content_type) = ctx.fetch_text_limited(playlist_url).await?;
    parse_playlist_body(&body, &base_url, &content_type)
}

pub fn parse_playlist_body(
    body: &str,
    base_url: &Url,
    content_type: &str,
) -> RuviResult<Vec<PlaylistEntry>> {
    let trimmed = body.trim();
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

    // M3U / plain line-based playlist
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
