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
    pub output: ManifestOutput,
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
        let major = manifest
            .schema_version
            .split('.')
            .next()
            .unwrap_or_default();
        if major != "1" {
            return Err(RuviError::Unsupported(format!(
                "unsupported manifest schema_version '{}' (expected major version '1')",
                manifest.schema_version
            )));
        }
        if manifest.output.path.trim().is_empty() {
            return Err(RuviError::InvalidArgument(
                "manifest output.path must not be empty".to_string(),
            ));
        }
        Ok(manifest)
    }
}
