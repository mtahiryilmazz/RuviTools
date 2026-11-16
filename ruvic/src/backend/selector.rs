use std::path::PathBuf;
use std::time::Duration;

use crate::backend::MediaBackend;
use crate::backend::ffmpeg::{FfmpegBackend, FfmpegLocations, locate_ffmpeg};
use crate::config::RuvicConfig;
use crate::error::RuviResult;

#[derive(Debug, Clone, Default)]
pub struct BackendOverrides {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub timeout_seconds: Option<u64>,
}

pub fn select_backend(
    config: &RuvicConfig,
    overrides: &BackendOverrides,
) -> RuviResult<Box<dyn MediaBackend>> {
    let locations: FfmpegLocations = locate_ffmpeg(
        overrides.ffmpeg.as_deref(),
        overrides.ffprobe.as_deref(),
        config.backend.ffmpeg_path.as_deref(),
        config.backend.ffprobe_path.as_deref(),
    )?;
    let timeout_secs = overrides
        .timeout_seconds
        .unwrap_or(config.backend.timeout_seconds);
    Ok(Box::new(FfmpegBackend::new(
        locations,
        Duration::from_secs(timeout_secs),
    )))
}
