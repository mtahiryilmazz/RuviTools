pub mod audio_transcode;
pub mod container_probe;
pub mod dual_mux;
pub mod ebml;
pub mod mp4_atoms;
pub mod processor;

use std::path::Path;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::backend::native::container_probe::probe_media_file;
use crate::backend::native::processor::execute_native_plan;
use crate::backend::{BackendCapabilities, MediaBackend, ProgressSink};
use crate::domain::conversion_job::ExecutionReport;
use crate::domain::media_info::MediaInfo;
use crate::error::RuviResult;
use crate::planner::ConversionPlan;

/// Built-in 100% pure-Rust media inspection and container/stream processor backend.
/// Requires zero external `ffmpeg` or `ffprobe` executables.
pub struct NativeMediaBackend;

impl NativeMediaBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn native_capabilities() -> BackendCapabilities {
        BackendCapabilities {
            ffmpeg_version: Some(format!(
                "ruvic-native {} (pure-rust)",
                env!("CARGO_PKG_VERSION")
            )),
            ffprobe_version: Some(format!(
                "ruvic-probe {} (pure-rust)",
                env!("CARGO_PKG_VERSION")
            )),
            encoders: vec![
                "libx264".to_string(),
                "libx265".to_string(),
                "libvpx-vp9".to_string(),
                "libaom-av1".to_string(),
                "aac".to_string(),
                "libmp3lame".to_string(),
                "flac".to_string(),
                "libopus".to_string(),
                "libvorbis".to_string(),
                "pcm_s16le".to_string(),
            ],
            hwaccels: Vec::new(),
        }
    }
}

impl Default for NativeMediaBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MediaBackend for NativeMediaBackend {
    async fn capabilities(&self) -> RuviResult<BackendCapabilities> {
        Ok(Self::native_capabilities())
    }

    async fn probe(&self, input: &Path) -> RuviResult<MediaInfo> {
        let probed = probe_media_file(input)?;
        Ok(probed.info)
    }

    async fn execute(
        &self,
        plan: &ConversionPlan,
        progress: &dyn ProgressSink,
        cancel: CancellationToken,
    ) -> RuviResult<ExecutionReport> {
        execute_native_plan(plan, progress, cancel).await
    }
}
