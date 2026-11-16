use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadCheckpoint {
    pub version: u32,
    pub url_sha256: String,
    pub stream_id: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub total_bytes: Option<u64>,
    pub chunk_size: u64,
    pub chunks_total: usize,
    pub completed_chunks: BTreeSet<usize>,
    pub completed_segments: BTreeSet<usize>,
}

impl DownloadCheckpoint {
    pub fn new(
        url: &str,
        stream_id: &str,
        etag: Option<String>,
        last_modified: Option<String>,
        total_bytes: Option<u64>,
        chunk_size: u64,
        chunks_total: usize,
    ) -> Self {
        Self {
            version: 1,
            url_sha256: sha256_hex(url.as_bytes()),
            stream_id: stream_id.to_string(),
            etag,
            last_modified,
            total_bytes,
            chunk_size,
            chunks_total,
            completed_chunks: BTreeSet::new(),
            completed_segments: BTreeSet::new(),
        }
    }

    pub fn load(path: &Path) -> RuviResult<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(path).map_err(|err| {
            RuviError::Filesystem(format!(
                "failed to read checkpoint {}: {err}",
                path.display()
            ))
        })?;
        let ckpt: Self = serde_json::from_str(&raw).map_err(|err| {
            RuviError::Integrity(format!("corrupted checkpoint {}: {err}", path.display()))
        })?;
        Ok(Some(ckpt))
    }

    /// Writes checkpoint atomically via temporary sibling file + rename.
    pub fn save_atomic(&self, path: &Path) -> RuviResult<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut tmp_os = path.as_os_str().to_os_string();
        tmp_os.push(".tmp");
        let tmp_path = PathBuf::from(tmp_os);
        let serialized = serde_json::to_string_pretty(self)
            .map_err(|err| RuviError::General(format!("serialize checkpoint: {err}")))?;
        std::fs::write(&tmp_path, serialized)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn verify_identity(
        &self,
        url: &str,
        stream_id: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        total_bytes: Option<u64>,
    ) -> RuviResult<()> {
        if self.url_sha256 != sha256_hex(url.as_bytes()) {
            return Err(RuviError::Integrity(
                "checkpoint URL mismatch; cannot safely resume".to_string(),
            ));
        }
        if self.stream_id != stream_id {
            return Err(RuviError::Integrity(
                "checkpoint stream ID mismatch; cannot safely resume".to_string(),
            ));
        }
        if let (Some(prev), Some(curr)) = (self.total_bytes, total_bytes)
            && prev != curr
        {
            return Err(RuviError::Integrity(format!(
                "remote content length changed ({prev} -> {curr}); refusing stale resume"
            )));
        }
        if let (Some(prev), Some(curr)) = (self.etag.as_deref(), etag)
            && prev != curr
        {
            return Err(RuviError::Integrity(format!(
                "remote ETag changed ({prev} -> {curr}); refusing stale resume"
            )));
        }
        if let (Some(prev), Some(curr)) = (self.last_modified.as_deref(), last_modified)
            && prev != curr
        {
            return Err(RuviError::Integrity(format!(
                "remote Last-Modified changed ({prev} -> {curr}); refusing stale resume"
            )));
        }
        Ok(())
    }
}

pub fn checkpoint_path_for(part_path: &Path) -> PathBuf {
    let mut os = part_path.as_os_str().to_os_string();
    os.push(".ruvid.json");
    PathBuf::from(os)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}
