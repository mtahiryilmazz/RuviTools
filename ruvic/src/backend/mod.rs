pub mod ffmpeg;
pub mod native;
pub mod selector;

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::domain::conversion_job::ExecutionReport;
use crate::domain::media_info::MediaInfo;
use crate::error::RuviResult;
use crate::planner::ConversionPlan;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BackendCapabilities {
    pub ffmpeg_version: Option<String>,
    pub ffprobe_version: Option<String>,
    pub encoders: Vec<String>,
    pub hwaccels: Vec<String>,
}

impl BackendCapabilities {
    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders.iter().any(|e| e.eq_ignore_ascii_case(name))
    }

    pub fn has_hwaccel(&self, name: &str) -> bool {
        self.hwaccels.iter().any(|h| h.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProgressUpdate {
    pub out_time_us: Option<u64>,
    pub fps: Option<f64>,
    pub speed: Option<String>,
    pub finished: bool,
}

pub trait ProgressSink: Send + Sync {
    fn on_progress(&self, update: ProgressUpdate);
}

#[derive(Debug, Default)]
pub struct NoopProgressSink;

impl ProgressSink for NoopProgressSink {
    fn on_progress(&self, _update: ProgressUpdate) {}
}

#[async_trait]
pub trait MediaBackend: Send + Sync {
    async fn capabilities(&self) -> RuviResult<BackendCapabilities>;
    async fn probe(&self, input: &Path) -> RuviResult<MediaInfo>;
    async fn execute(
        &self,
        plan: &ConversionPlan,
        progress: &dyn ProgressSink,
        cancel: CancellationToken,
    ) -> RuviResult<ExecutionReport>;
}
