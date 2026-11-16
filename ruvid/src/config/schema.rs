use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct RuvidConfig {
    pub download: DownloadConfig,
    pub network: NetworkConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DownloadConfig {
    pub output_dir: PathBuf,
    pub jobs: usize,
    pub resume: bool,
    pub overwrite: bool,
    pub retry_count: u32,
    pub timeout_seconds: u64,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./Downloads"),
            jobs: 4,
            resume: true,
            overwrite: false,
            retry_count: 5,
            timeout_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NetworkConfig {
    pub user_agent: String,
    pub connect_timeout_seconds: u64,
    pub limit_rate: String,
    pub proxy: Option<String>,
    pub allow_private_network: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
            connect_timeout_seconds: 10,
            limit_rate: String::new(),
            proxy: None,
            allow_private_network: false,
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
