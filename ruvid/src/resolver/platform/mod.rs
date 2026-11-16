use async_trait::async_trait;
use url::Url;

use crate::domain::media_source::MediaSource;
use crate::error::RuviResult;
use crate::resolver::ResolveContext;

#[async_trait]
pub trait PlatformResolver: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_handle(&self, url: &Url, content_type: &str) -> bool;
    async fn resolve(
        &self,
        ctx: &ResolveContext,
        original_url: &str,
        final_url: &Url,
        html_body: &str,
    ) -> RuviResult<Option<String>>;
}

pub struct HtmlMetadataPlatformResolver;

#[async_trait]
impl PlatformResolver for HtmlMetadataPlatformResolver {
    fn name(&self) -> &'static str {
        "html_metadata"
    }

    fn can_handle(&self, _url: &Url, content_type: &str) -> bool {
        content_type.contains("text/html") || content_type.contains("application/xhtml+xml")
    }

    async fn resolve(
        &self,
        _ctx: &ResolveContext,
        _original_url: &str,
        final_url: &Url,
        html_body: &str,
    ) -> RuviResult<Option<String>> {
        if let Some(found) = extract_media_url_from_html(html_body)
            && let Ok(joined) = final_url.join(&found)
        {
            return Ok(Some(joined.to_string()));
        }
        Ok(None)
    }
}

pub fn extract_title_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let after = &html[start + 7..];
    let end = after.to_ascii_lowercase().find("</title>")?;
    let title = after[..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

pub fn extract_media_url_from_html(html: &str) -> Option<String> {
    // 1. OpenGraph og:video / og:video:url / og:video:secure_url
    for marker in [
        "property=\"og:video:secure_url\"",
        "property=\"og:video:url\"",
        "property=\"og:video\"",
        "property='og:video'",
    ] {
        if let Some(pos) = html.find(marker) {
            let tag_start = html[..pos].rfind('<').unwrap_or(pos);
            if let Some(tag_end) = html[pos..].find('>') {
                let tag = &html[tag_start..pos + tag_end];
                if let Some(content) = extract_attr(tag, "content") {
                    return Some(content);
                }
            }
        }
    }

    // 2. HTML5 <video src="..."> or <source src="...">
    for tag_name in ["<video", "<source"] {
        if let Some(pos) = html.to_ascii_lowercase().find(tag_name)
            && let Some(end) = html[pos..].find('>')
        {
            let tag = &html[pos..pos + end];
            if let Some(src) = extract_attr(tag, "src") {
                return Some(src);
            }
        }
    }

    // 3. JSON-LD "contentUrl": "..."
    if let Some(pos) = html.find("\"contentUrl\"") {
        let after = &html[pos + "\"contentUrl\"".len()..];
        if let Some(colon) = after.find(':') {
            let after_colon = after[colon + 1..].trim_start();
            if let Some(quoted) = after_colon.strip_prefix('"')
                && let Some(end) = quoted.find('"')
            {
                return Some(quoted[..end].to_string());
            }
        }
    }

    None
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{attr_name}={quote}");
        if let Some(pos) = tag.find(&needle) {
            let after = &tag[pos + needle.len()..];
            if let Some(end) = after.find(quote) {
                let val = after[..end].trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub fn with_html_title(mut source: MediaSource, title: Option<String>) -> MediaSource {
    if let Some(t) = title {
        source.title = t;
    }
    source
}
