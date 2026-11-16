use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::RuviResult;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn create_temp_output_path(final_output: &Path) -> RuviResult<PathBuf> {
    let parent = final_output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = final_output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ruvic");
    let ext = final_output
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("tmp");
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    Ok(parent.join(format!(".{stem}.ruvic-tmp-{pid}-{seq}.{ext}")))
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
        if !self.committed && self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
