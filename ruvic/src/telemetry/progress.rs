use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::backend::{ProgressSink, ProgressUpdate};

pub struct CliProgressSink {
    bar: Option<Mutex<ProgressBar>>,
    total_duration_us: Option<u64>,
}

impl CliProgressSink {
    pub fn new(enabled: bool, total_duration_seconds: Option<f64>, label: &str) -> Self {
        let total_duration_us = total_duration_seconds
            .filter(|d| *d > 0.0)
            .map(|d| (d * 1_000_000.0) as u64);

        if !enabled {
            return Self {
                bar: None,
                total_duration_us,
            };
        }

        let bar = match total_duration_us {
            Some(total) => ProgressBar::with_draw_target(Some(total), ProgressDrawTarget::stderr()),
            None => ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr()),
        };

        let template = if total_duration_us.is_some() {
            "{prefix:.bold} [{bar:30.cyan/dim}] {percent:>3}% {msg}"
        } else {
            "{spinner:.cyan} {prefix:.bold} {msg}"
        };
        if let Ok(style) = ProgressStyle::with_template(template) {
            bar.set_style(style.progress_chars("##-"));
        }
        bar.set_prefix(label.to_string());

        Self {
            bar: Some(Mutex::new(bar)),
            total_duration_us,
        }
    }

    pub fn finish(&self) {
        if let Some(ref mutex) = self.bar
            && let Ok(bar) = mutex.lock()
        {
            bar.finish_and_clear();
        }
    }
}

impl ProgressSink for CliProgressSink {
    fn on_progress(&self, update: ProgressUpdate) {
        let Some(ref mutex) = self.bar else {
            return;
        };
        let Ok(bar) = mutex.lock() else {
            return;
        };
        if let (Some(total), Some(cur)) = (self.total_duration_us, update.out_time_us) {
            bar.set_position(cur.min(total));
        } else {
            bar.tick();
        }
        let mut details = Vec::new();
        if let Some(fps) = update.fps {
            details.push(format!("{fps:.1} fps"));
        }
        if let Some(ref speed) = update.speed {
            details.push(speed.clone());
        }
        if !details.is_empty() {
            bar.set_message(details.join(" | "));
        }
        if update.finished {
            bar.finish_and_clear();
        }
    }
}
