pub mod master_playlist;
pub mod media_playlist;
pub mod parser;

use url::Url;

use crate::domain::media_source::{MediaSource, SourceKind};
use crate::error::{RuviError, RuviResult};
use crate::resolver::ResolveContext;

pub const MAX_HLS_DEPTH: u8 = 3;

pub async fn resolve_hls(
    _ctx: &ResolveContext,
    original_url: &str,
    playlist_url: &Url,
    body: &str,
    depth: u8,
) -> RuviResult<MediaSource> {
    if depth > MAX_HLS_DEPTH {
        return Err(RuviError::Unsupported(format!(
            "HLS playlist recursion depth exceeded maximum ({MAX_HLS_DEPTH})"
        )));
    }

    if !body.contains("#EXTM3U") {
        return Err(RuviError::Unsupported(
            "invalid HLS playlist: missing #EXTM3U header".to_string(),
        ));
    }

    if master_playlist::is_master_playlist(body) {
        master_playlist::parse_master_playlist(original_url, playlist_url, body)
    } else {
        let parsed = media_playlist::parse_media_playlist(playlist_url, body)?;
        let title = derive_hls_title(playlist_url);
        let stream = media_playlist::build_single_stream(playlist_url, &parsed);
        Ok(MediaSource {
            url: original_url.to_string(),
            resolved_url: playlist_url.to_string(),
            kind: SourceKind::HlsVod,
            title,
            default_ext: stream.container.clone(),
            streams: vec![stream],
            subtitles: Vec::new(),
        })
    }
}

pub async fn fetch_and_parse_media_playlist(
    ctx: &ResolveContext,
    playlist_url: &str,
) -> RuviResult<media_playlist::ParsedMediaPlaylist> {
    let (body, final_url, _) = ctx.fetch_text_limited(playlist_url).await?;
    if !body.contains("#EXTM3U") {
        return Err(RuviError::Unsupported(
            "invalid HLS media playlist: missing #EXTM3U header".to_string(),
        ));
    }
    media_playlist::parse_media_playlist(&final_url, &body)
}

pub fn derive_hls_title(url: &Url) -> String {
    url.path_segments()
        .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
        .and_then(|name| name.strip_suffix(".m3u8").or(Some(name)))
        .filter(|s| !s.is_empty())
        .unwrap_or("hls_stream")
        .to_string()
}
