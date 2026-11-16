use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
    LAST_MODIFIED, RANGE,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::domain::media_source::{MediaSource, SourceKind};
use crate::domain::stream::{MediaStream, StreamKind, StreamPayload};
use crate::downloader::retry::{RetryableError, run_with_retry};
use crate::error::RuviResult;
use crate::resolver::{ResolveContext, validate_url_security};

#[derive(Debug, Clone)]
pub struct HttpProbeMetadata {
    pub final_url: Url,
    pub total_bytes: Option<u64>,
    pub accept_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition_filename: Option<String>,
}

pub async fn probe_http_resource(
    ctx: &ResolveContext,
    raw_url: &str,
) -> RuviResult<HttpProbeMetadata> {
    let parsed = ctx.validate_url(raw_url)?;
    let cancel = CancellationToken::new();

    run_with_retry(ctx.retry_count, &cancel, || {
        let parsed_c = parsed.clone();
        async move {
            // Controlled GET with Range: bytes=0-0 (avoids unreliable HEAD behavior on some servers)
            let resp = ctx
                .client
                .get(parsed_c.clone())
                .header(RANGE, "bytes=0-0")
                .send()
                .await
                .map_err(RetryableError::from_reqwest)?;

            let final_url = resp.url().clone();
            validate_url_security(&final_url, ctx.allow_private_net)
                .map_err(RetryableError::Permanent)?;

            let status = resp.status();
            if status == StatusCode::RANGE_NOT_SATISFIABLE
                || status == StatusCode::METHOD_NOT_ALLOWED
            {
                let fallback = ctx
                    .client
                    .get(parsed_c)
                    .send()
                    .await
                    .map_err(RetryableError::from_reqwest)?;
                let fb_url = fallback.url().clone();
                validate_url_security(&fb_url, ctx.allow_private_net)
                    .map_err(RetryableError::Permanent)?;
                if !fallback.status().is_success() {
                    return Err(RetryableError::from_status(
                        fallback.status(),
                        fb_url.as_str(),
                    ));
                }
                return Ok(extract_metadata_from_response(&fallback, fb_url, false));
            }

            if !status.is_success() {
                return Err(RetryableError::from_status(status, final_url.as_str()));
            }

            let is_partial = status == StatusCode::PARTIAL_CONTENT;
            Ok(extract_metadata_from_response(&resp, final_url, is_partial))
        }
    })
    .await
}

fn extract_metadata_from_response(
    resp: &reqwest::Response,
    final_url: Url,
    is_partial: bool,
) -> HttpProbeMetadata {
    let headers = resp.headers();

    let content_range_total = headers
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range_total);

    let header_len = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());

    let total_bytes = if is_partial {
        content_range_total
    } else {
        header_len
    };

    let accept_ranges_header = headers
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("bytes"));

    // Require either 206 with valid Content-Range total OR explicit Accept-Ranges: bytes when known total exists
    let accept_ranges = is_partial && content_range_total.is_some()
        || (accept_ranges_header && total_bytes.is_some());

    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());
    let last_modified = headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase());

    let content_disposition_filename = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename);

    HttpProbeMetadata {
        final_url,
        total_bytes,
        accept_ranges,
        etag,
        last_modified,
        content_type,
        content_disposition_filename,
    }
}

pub fn parse_content_range_total(header_val: &str) -> Option<u64> {
    // Format: "bytes 0-0/12345" or "bytes 0-1023/*"
    let (_, total_part) = header_val.trim().split_once('/')?;
    if total_part.trim() == "*" {
        return None;
    }
    total_part.trim().parse::<u64>().ok()
}

fn parse_content_disposition_filename(header_val: &str) -> Option<String> {
    for part in header_val.split(';') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix("filename=") {
            let unquoted = rest.trim().trim_matches('"');
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

pub fn resolve_direct_http_from_probe(original_url: &str, meta: HttpProbeMetadata) -> MediaSource {
    let (title, ext) = derive_title_and_ext(
        &meta.final_url,
        meta.content_disposition_filename.as_deref(),
        meta.content_type.as_deref(),
    );

    let stream = MediaStream {
        id: "direct-0".to_string(),
        kind: StreamKind::VideoAudio,
        container: ext.clone(),
        width: None,
        height: None,
        bandwidth_bps: None,
        codecs: None,
        frame_rate: None,
        language: None,
        payload: StreamPayload::DirectHttp {
            url: meta.final_url.to_string(),
            total_bytes: meta.total_bytes,
            accept_ranges: meta.accept_ranges,
            etag: meta.etag,
            last_modified: meta.last_modified,
        },
    };

    MediaSource {
        url: original_url.to_string(),
        resolved_url: meta.final_url.to_string(),
        kind: SourceKind::DirectHttp,
        title,
        default_ext: ext,
        streams: vec![stream],
        subtitles: Vec::new(),
    }
}

fn derive_title_and_ext(
    url: &Url,
    disposition_filename: Option<&str>,
    content_type: Option<&str>,
) -> (String, String) {
    let raw_name = disposition_filename
        .map(ToString::to_string)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "media".to_string());

    if let Some((stem, ext)) = raw_name.rsplit_once('.')
        && !stem.is_empty()
        && !ext.is_empty()
        && ext.len() <= 8
    {
        return (stem.to_string(), ext.to_ascii_lowercase());
    }

    let ext = match content_type {
        Some("video/mp4") => "mp4",
        Some("video/webm") => "webm",
        Some("video/x-matroska") => "mkv",
        Some("video/quicktime") => "mov",
        Some("audio/mpeg") => "mp3",
        Some("audio/mp4") => "m4a",
        Some("audio/flac") => "flac",
        _ => "bin",
    };

    (raw_name, ext.to_string())
}
