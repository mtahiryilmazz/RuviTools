use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::stream::StreamInfo;
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaInfo {
    pub path: String,
    pub container: ContainerInfo,
    pub streams: Vec<StreamInfo>,
    #[serde(default)]
    pub chapters: Vec<ChapterInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerInfo {
    pub format_name: String,
    pub format_long_name: Option<String>,
    pub duration_seconds: Option<f64>,
    pub size_bytes: Option<u64>,
    pub bit_rate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChapterInfo {
    pub id: i64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub title: Option<String>,
}

/// RUVI JSON Handoff Manifest (`schema_version = "1"`) written by `ruvid` and read by `ruvic`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandoffManifest {
    pub schema_version: String,
    #[serde(default)]
    pub producer: Option<ManifestProducer>,
    #[serde(default)]
    pub source: Option<ManifestSource>,
    #[serde(default)]
    pub output: Option<ManifestOutput>,
    /// Dual-stream (DASH) handoff: primary (video) input path.
    #[serde(default)]
    pub primary_input: Option<String>,
    /// Dual-stream (DASH) handoff: secondary (audio) input path.
    #[serde(default)]
    pub secondary_input: Option<String>,
    #[serde(default)]
    pub streams: Vec<ManifestStream>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestProducer {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestSource {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestOutput {
    pub path: String,
    pub container: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestStream {
    pub kind: String,
    pub codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub language: Option<String>,
}

impl HandoffManifest {
    pub fn load_from_path(path: &Path) -> RuviResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|err| {
            RuviError::Filesystem(format!("failed to read manifest {}: {err}", path.display()))
        })?;
        let manifest: Self = serde_json::from_str(&content).map_err(|err| {
            RuviError::InvalidArgument(format!("invalid RUVI manifest {}: {err}", path.display()))
        })?;
        if !is_supported_manifest_version(&manifest.schema_version) {
            return Err(RuviError::Unsupported(format!(
                "unsupported manifest schema_version '{}' (expected major version '1')",
                manifest.schema_version
            )));
        }
        if manifest.primary_path().is_none() {
            return Err(RuviError::InvalidArgument(
                "manifest must provide a non-empty output.path or primary_input".to_string(),
            ));
        }
        Ok(manifest)
    }

    /// Primary media path: `primary_input` (dual-stream) or `output.path` (single file).
    pub fn primary_path(&self) -> Option<&str> {
        self.primary_input
            .as_deref()
            .or(self.output.as_ref().map(|o| o.path.as_str()))
            .map(str::trim)
            .filter(|p| !p.is_empty())
    }

    /// Secondary (audio) media path for dual-stream handoffs.
    pub fn secondary_path(&self) -> Option<&str> {
        self.secondary_input
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
    }
}

fn is_supported_manifest_version(version: &str) -> bool {
    let mut parts = version.trim().split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    major == "1" && parts.all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}
