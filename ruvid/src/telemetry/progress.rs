use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

const UNKNOWN_TOTAL: u64 = u64::MAX;

pub type ProgressCallback = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

#[derive(Clone)]
pub struct DownloadProgress {
    bar: Option<ProgressBar>,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    callback: Option<ProgressCallback>,
    last_cb_ms: Arc<AtomicU64>,
    started_at: Instant,
}

impl DownloadProgress {
    pub fn new(enabled: bool, total_bytes: Option<u64>, label: &str) -> Self {
        Self::with_callback(enabled, total_bytes, label, None)
    }

    pub fn with_callback(
        enabled: bool,
        total_bytes: Option<u64>,
        label: &str,
        callback: Option<ProgressCallback>,
    ) -> Self {
        let bar = if enabled {
            let b = match total_bytes {
                Some(total) => {
                    ProgressBar::with_draw_target(Some(total), ProgressDrawTarget::stderr())
                }
                None => ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr()),
            };
            let tpl = if total_bytes.is_some() {
                "{prefix:.bold} [{bar:30.green/dim}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
            } else {
                "{spinner:.green} {prefix:.bold} {bytes} ({bytes_per_sec})"
            };
            if let Ok(style) = ProgressStyle::with_template(tpl) {
                b.set_style(style.progress_chars("##-"));
            }
            b.set_prefix(label.to_string());
            Some(b)
        } else {
            None
        };

        Self {
            bar,
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(total_bytes.unwrap_or(UNKNOWN_TOTAL))),
            callback,
            last_cb_ms: Arc::new(AtomicU64::new(0)),
            started_at: Instant::now(),
        }
    }

    pub fn set_total(&self, total_bytes: u64) {
        self.total.store(total_bytes, Ordering::Relaxed);
        if let Some(ref bar) = self.bar {
            bar.set_length(total_bytes);
            let tpl = "{prefix:.bold} [{bar:30.green/dim}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})";
            if let Ok(style) = ProgressStyle::with_template(tpl) {
                bar.set_style(style.progress_chars("##-"));
            }
        }
        if let Some(ref cb) = self.callback {
            cb(self.downloaded.load(Ordering::Relaxed), Some(total_bytes));
        }
    }

    pub fn total(&self) -> Option<u64> {
        let val = self.total.load(Ordering::Relaxed);
        if val == UNKNOWN_TOTAL {
            None
        } else {
            Some(val)
        }
    }

    pub fn inc(&self, delta: u64) {
        let total = self.downloaded.fetch_add(delta, Ordering::Relaxed) + delta;
        if let Some(ref bar) = self.bar {
            bar.set_position(total);
        }
        if let Some(ref cb) = self.callback {
            let now_ms = self.started_at.elapsed().as_millis() as u64;
            let prev = self.last_cb_ms.load(Ordering::Relaxed);
            if now_ms.saturating_sub(prev) >= 200
                && self
                    .last_cb_ms
                    .compare_exchange(prev, now_ms, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                cb(total, self.total());
            }
        }
    }

    pub fn set_initial(&self, initial: u64) {
        self.downloaded.store(initial, Ordering::Relaxed);
        if let Some(ref bar) = self.bar {
            bar.set_position(initial);
        }
    }

    pub fn finish(&self) {
        if let Some(ref cb) = self.callback {
            cb(self.downloaded.load(Ordering::Relaxed), self.total());
        }
        if let Some(ref bar) = self.bar {
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_callback_emits_on_set_total_and_finish() {
        let events = Arc::new(std::sync::Mutex::new(Vec::<(u64, Option<u64>)>::new()));
        let events_c = events.clone();
        let cb: ProgressCallback = Arc::new(move |done, total| {
            if let Ok(mut g) = events_c.lock() {
                g.push((done, total));
            }
        });
        let p = DownloadProgress::with_callback(false, None, "test", Some(cb));
        p.set_total(1000);
        p.inc(500);
        p.finish();
        let captured = events.lock().unwrap();
        assert!(captured.len() >= 2);
        assert_eq!(captured[0], (0, Some(1000)));
        assert_eq!(*captured.last().unwrap(), (500, Some(1000)));
    }
}
