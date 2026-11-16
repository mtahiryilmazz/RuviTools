use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::RuviResult;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn create_temp_output_path(final_output: &Path) -> RuviResult<PathBuf> {
    let parent = final_output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let raw_stem = final_output
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("ruvic");
    let stem = truncate_utf8_prefix(raw_stem, 64);
    let ext = final_output
        .extension()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("tmp");
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    Ok(parent.join(format!(".{stem}.ruvic-tmp-{pid}-{seq}.{ext}")))
}

fn truncate_utf8_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 { "ruvic" } else { &s[..end] }
}

/// Removes the temporary output file on drop unless explicitly committed.
pub struct TempOutputGuard {
    path: PathBuf,
    committed: bool,
}

impl TempOutputGuard {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mark_committed(&mut self) {
        self.committed = true;
    }
}

impl Drop for TempOutputGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
