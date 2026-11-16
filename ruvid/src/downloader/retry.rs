use std::time::Duration;

use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;

use crate::error::{RuviError, RuviResult};

pub fn is_transient_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

pub fn backoff_duration(attempt: u32) -> Duration {
    let base_ms: u64 = std::env::var("RUVID_RETRY_BASE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let factor = 1u64.checked_shl(attempt.min(6)).unwrap_or(64);
    Duration::from_millis((base_ms * factor).min(5000))
}

pub async fn run_with_retry<T, F, Fut>(
    max_retries: u32,
    cancel: &CancellationToken,
    mut op: F,
) -> RuviResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, RetryableError>>,
{
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            return Err(RuviError::Cancelled("download cancelled".to_string()));
        }
        match op().await {
            Ok(v) => return Ok(v),
            Err(RetryableError::Permanent(err)) => return Err(err),
            Err(RetryableError::Transient(err)) => {
                if attempt >= max_retries {
                    return Err(err);
                }
                let delay = backoff_duration(attempt);
                attempt += 1;
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = cancel.cancelled() => {
                        return Err(RuviError::Cancelled("download cancelled during retry wait".to_string()));
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum RetryableError {
    Transient(RuviError),
    Permanent(RuviError),
}

impl RetryableError {
    pub fn from_reqwest(err: reqwest::Error) -> Self {
        if err.is_timeout() || err.is_connect() || err.is_request() {
            Self::Transient(RuviError::Network(err.to_string()))
        } else {
            Self::Permanent(RuviError::Network(err.to_string()))
        }
    }

    pub fn from_status(status: StatusCode, url: &str) -> Self {
        let err = RuviError::Network(format!("HTTP {} for {}", status.as_u16(), url));
        if is_transient_status(status) {
            Self::Transient(err)
        } else {
            Self::Permanent(err)
        }
    }
}
