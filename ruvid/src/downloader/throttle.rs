use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::error::{RuviError, RuviResult};

pub fn parse_rate_limit(raw: &str) -> RuviResult<Option<u64>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let upper = trimmed.to_ascii_uppercase();
    let (num_part, multiplier) = if let Some(n) = upper.strip_suffix('G') {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = upper.strip_suffix('M') {
        (n, 1024 * 1024)
    } else if let Some(n) = upper.strip_suffix('K') {
        (n, 1024)
    } else if let Some(n) = upper.strip_suffix('B') {
        (n, 1)
    } else {
        (upper.as_str(), 1)
    };
    let val = num_part.trim().parse::<u64>().map_err(|_| {
        RuviError::InvalidArgument(format!(
            "invalid rate limit '{raw}' (expected e.g. 5M, 500K, 1048576)"
        ))
    })?;
    if val == 0 {
        return Err(RuviError::InvalidArgument(
            "rate limit must be greater than 0".to_string(),
        ));
    }
    Ok(Some(val.saturating_mul(multiplier)))
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Option<Arc<Mutex<BucketState>>>,
}

struct BucketState {
    rate_bps: f64,
    capacity: f64,
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(rate_bps: Option<u64>) -> Self {
        let inner = rate_bps.map(|rate| {
            let r = rate as f64;
            // Allow burst up to 1 second or 64 KiB minimum
            let cap = r.max(65_536.0);
            Arc::new(Mutex::new(BucketState {
                rate_bps: r,
                capacity: cap,
                tokens: cap.min(r),
                last: Instant::now(),
            }))
        });
        Self { inner }
    }

    pub async fn acquire(&self, bytes: usize) {
        let Some(ref mutex) = self.inner else {
            return;
        };
        if bytes == 0 {
            return;
        }
        let needed = bytes as f64;
        loop {
            let wait_dur = {
                let mut state = mutex.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(state.last).as_secs_f64();
                state.last = now;
                state.tokens = (state.tokens + elapsed * state.rate_bps).min(state.capacity);
                if state.tokens >= needed {
                    state.tokens -= needed;
                    None
                } else {
                    let deficit = needed - state.tokens;
                    Some(Duration::from_secs_f64(
                        (deficit / state.rate_bps).max(0.005),
                    ))
                }
            };
            match wait_dur {
                None => return,
                Some(dur) => tokio::time::sleep(dur).await,
            }
        }
    }
}
