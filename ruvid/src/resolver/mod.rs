pub mod dash;
pub mod direct_http;
pub mod hls;
pub mod platform;
pub mod registry;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use url::Url;

use crate::domain::media_manifest::redact_sensitive_url;
use crate::error::{RuviError, RuviResult};

pub const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PLAYLIST_SEGMENTS: usize = 20_000;
pub const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone)]
pub struct ResolveContext {
    pub client: reqwest::Client,
    pub custom_headers: HeaderMap,
    pub allow_private_net: bool,
    pub timeout: Duration,
    pub retry_count: u32,
}

impl ResolveContext {
    pub fn build(
        user_agent: &str,
        connect_timeout_secs: u64,
        request_timeout_secs: u64,
        proxy: Option<&str>,
        raw_headers: &[(String, String)],
        allow_private_net: bool,
        retry_count: u32,
    ) -> RuviResult<Self> {
        let allow_priv =
            allow_private_net || std::env::var("RUVID_ALLOW_PRIVATE_NET").as_deref() == Ok("1");

        let mut headers = HeaderMap::new();
        for (k, v) in raw_headers {
            let name = HeaderName::from_bytes(k.trim().as_bytes()).map_err(|err| {
                RuviError::InvalidArgument(format!("invalid header name '{k}': {err}"))
            })?;
            let val = HeaderValue::from_str(v.trim()).map_err(|err| {
                RuviError::InvalidArgument(format!("invalid header value for '{k}': {err}"))
            })?;
            headers.insert(name, val);
        }

        let policy_allow_priv = allow_priv;
        let redirect_policy = Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            if let Err(err) = validate_url_security(attempt.url(), policy_allow_priv) {
                return attempt.error(err.to_string());
            }
            attempt.follow()
        });

        let mut builder = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(connect_timeout_secs))
            .timeout(Duration::from_secs(request_timeout_secs))
            .redirect(redirect_policy)
            .default_headers(headers.clone());

        if let Some(proxy_url) = proxy {
            let p = reqwest::Proxy::all(proxy_url).map_err(|err| {
                RuviError::InvalidArgument(format!(
                    "invalid proxy URL '{}': {err}",
                    redact_sensitive_url(proxy_url)
                ))
            })?;
            builder = builder.proxy(p);
        }

        let client = builder
            .build()
            .map_err(|err| RuviError::Network(format!("failed to construct HTTP client: {err}")))?;

        Ok(Self {
            client,
            custom_headers: headers,
            allow_private_net: allow_priv,
            timeout: Duration::from_secs(request_timeout_secs),
            retry_count,
        })
    }

    pub fn validate_url(&self, raw: &str) -> RuviResult<Url> {
        let parsed = Url::parse(raw).map_err(|err| {
            RuviError::Unsupported(format!(
                "invalid URL '{}': {err}",
                redact_sensitive_url(raw)
            ))
        })?;
        validate_url_security(&parsed, self.allow_private_net)?;
        Ok(parsed)
    }

    pub async fn fetch_text_limited(&self, url: &str) -> RuviResult<(String, Url, String)> {
        let parsed = self.validate_url(url)?;
        let cancel = tokio_util::sync::CancellationToken::new();
        crate::downloader::retry::run_with_retry(self.retry_count, &cancel, || {
            let parsed_c = parsed.clone();
            async move {
                let resp = self
                    .client
                    .get(parsed_c)
                    .send()
                    .await
                    .map_err(crate::downloader::retry::RetryableError::from_reqwest)?;
                let final_url = resp.url().clone();
                validate_url_security(&final_url, self.allow_private_net)
                    .map_err(crate::downloader::retry::RetryableError::Permanent)?;

                let status = resp.status();
                if !status.is_success() {
                    return Err(crate::downloader::retry::RetryableError::from_status(
                        status,
                        final_url.as_str(),
                    ));
                }

                let content_type = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                let text = read_response_text_limited(resp, MAX_MANIFEST_BYTES)
                    .await
                    .map_err(crate::downloader::retry::RetryableError::Permanent)?;
                Ok((text, final_url, content_type))
            }
        })
        .await
    }
}

pub async fn read_response_text_limited(
    resp: reqwest::Response,
    max_bytes: usize,
) -> RuviResult<String> {
    let bytes = read_response_bytes_limited(resp, max_bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn read_response_bytes_limited(
    resp: reqwest::Response,
    max_bytes: usize,
) -> RuviResult<Vec<u8>> {
    if let Some(len) = resp.content_length()
        && len as usize > max_bytes
    {
        return Err(RuviError::Unsupported(format!(
            "response exceeds maximum allowed size ({max_bytes} bytes)"
        )));
    }

    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(crate::downloader::retry::map_reqwest_error)?;
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(RuviError::Unsupported(format!(
                "response body exceeds {max_bytes} bytes"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

pub fn validate_url_security(url: &Url, allow_private_net: bool) -> RuviResult<()> {
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(RuviError::Unsupported(format!(
                "unsupported URL scheme '{other}' (only http and https are allowed)"
            )));
        }
    }

    let host = url
        .host_str()
        .ok_or_else(|| RuviError::Unsupported("URL is missing a host".to_string()))?;

    if !allow_private_net && is_disallowed_host(host) {
        return Err(RuviError::Unsupported(format!(
            "refusing access to private/loopback host '{host}' (pass --allow-private-net to permit)"
        )));
    }

    Ok(())
}

fn is_disallowed_host(host: &str) -> bool {
    let normalized = host
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if normalized == "localhost"
        || normalized == "localhost.localdomain"
        || normalized == "ip6-localhost"
        || normalized == "ip6-loopback"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
    {
        return true;
    }
    if let Ok(ip) = normalized.parse::<IpAddr>() {
        return is_private_ip(ip);
    }
    false
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 0
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || (octets[0] == 100 && (octets[1] & 0xC0) == 64) // CGNAT 100.64.0.0/10
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
    {
        return true;
    }
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_private_ipv4(v4);
    }
    let segs = ip.segments();
    // Deprecated site-local fec0::/10
    if (segs[0] & 0xffc0) == 0xfec0 {
        return true;
    }
    // IPv4-compatible IPv6 ::/96 and NAT64 64:ff9b::/96
    if segs[..6] == [0, 0, 0, 0, 0, 0] || segs[..6] == [0x0064, 0xff9b, 0, 0, 0, 0] {
        let v4 = Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            segs[6] as u8,
            (segs[7] >> 8) as u8,
            segs[7] as u8,
        );
        return is_private_ipv4(v4);
    }
    false
}
