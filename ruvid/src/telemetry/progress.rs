use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

#[derive(Clone)]
pub struct DownloadProgress {
    bar: Option<ProgressBar>,
    downloaded: Arc<AtomicU64>,
}

impl DownloadProgress {
    pub fn new(enabled: bool, total_bytes: Option<u64>, label: &str) -> Self {
        if !enabled {
            return Self {
                bar: None,
                downloaded: Arc::new(AtomicU64::new(0)),
            };
        }

        let bar = match total_bytes {
            Some(total) => ProgressBar::with_draw_target(Some(total), ProgressDrawTarget::stderr()),
            None => ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr()),
        };

        let tpl = if total_bytes.is_some() {
            "{prefix:.bold} [{bar:30.green/dim}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
        } else {
            "{spinner:.green} {prefix:.bold} {bytes} ({bytes_per_sec})"
        };
        if let Ok(style) = ProgressStyle::with_template(tpl) {
            bar.set_style(style.progress_chars("##-"));
        }
        bar.set_prefix(label.to_string());

        Self {
            bar: Some(bar),
            downloaded: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn inc(&self, delta: u64) {
        let total = self.downloaded.fetch_add(delta, Ordering::Relaxed) + delta;
        if let Some(ref bar) = self.bar {
            bar.set_position(total);
        }
    }

    pub fn set_initial(&self, initial: u64) {
        self.downloaded.store(initial, Ordering::Relaxed);
        if let Some(ref bar) = self.bar {
            bar.set_position(initial);
        }
    }

    pub fn finish(&self) {
        if let Some(ref bar) = self.bar {
            bar.finish_and_clear();
        }
    }
}
