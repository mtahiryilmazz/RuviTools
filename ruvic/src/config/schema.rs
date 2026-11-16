use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct RuvicConfig {
    pub backend: BackendConfig,
    pub processing: ProcessingConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BackendConfig {
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
    pub timeout_seconds: u64,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: None,
            ffprobe_path: None,
            timeout_seconds: 1800,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProcessingConfig {
    pub overwrite: bool,
    pub hwaccel: String,
    pub threads: Option<u32>,
    pub default_preset: Option<String>,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            overwrite: false,
            hwaccel: "auto".to_string(),
            threads: None,
            default_preset: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutputConfig {
    pub progress: bool,
    pub color: String,
    pub json: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            progress: true,
            color: "auto".to_string(),
            json: false,
        }
    }
}
