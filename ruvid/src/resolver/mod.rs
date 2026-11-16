pub mod dash;
pub mod direct_http;
pub mod hls;
pub mod platform;
pub mod registry;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use url::Url;

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
                RuviError::InvalidArgument(format!("invalid proxy URL '{proxy_url}': {err}"))
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
        let parsed = Url::parse(raw)
            .map_err(|err| RuviError::Unsupported(format!("invalid URL '{raw}': {err}")))?;
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

                if let Some(len) = resp.content_length()
                    && len as usize > MAX_MANIFEST_BYTES
                {
                    return Err(crate::downloader::retry::RetryableError::Permanent(
                        RuviError::Unsupported(format!(
                            "manifest exceeds maximum allowed size ({MAX_MANIFEST_BYTES} bytes)"
                        )),
                    ));
                }

                let bytes = resp
                    .bytes()
                    .await
                    .map_err(crate::downloader::retry::RetryableError::from_reqwest)?;
                if bytes.len() > MAX_MANIFEST_BYTES {
                    return Err(crate::downloader::retry::RetryableError::Permanent(
                        RuviError::Unsupported(format!(
                            "manifest body exceeds {MAX_MANIFEST_BYTES} bytes"
                        )),
                    ));
                }

                let text = String::from_utf8_lossy(&bytes).into_owned();
                Ok((text, final_url, content_type))
            }
        })
        .await
    }
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
    let lower = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = lower.parse::<IpAddr>() {
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
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xC0) == 64) // CGNAT 100.64.0.0/10
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
}
