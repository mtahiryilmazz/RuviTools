use std::path::PathBuf;
use std::time::Duration;

use crate::backend::MediaBackend;
use crate::backend::ffmpeg::{FfmpegBackend, FfmpegLocations, locate_ffmpeg};
use crate::backend::native::NativeMediaBackend;
use crate::config::RuvicConfig;
use crate::error::RuviResult;

#[derive(Debug, Clone, Default)]
pub struct BackendOverrides {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub timeout_seconds: Option<u64>,
}

impl BackendOverrides {
    pub fn has_explicit_ffmpeg(&self, config: &RuvicConfig) -> bool {
        self.ffmpeg
            .as_ref()
            .is_some_and(|p| !p.as_os_str().is_empty())
            || self
                .ffprobe
                .as_ref()
                .is_some_and(|p| !p.as_os_str().is_empty())
            || config
                .backend
                .ffmpeg_path
                .as_ref()
                .is_some_and(|p| !p.as_os_str().is_empty())
            || config
                .backend
                .ffprobe_path
                .as_ref()
                .is_some_and(|p| !p.as_os_str().is_empty())
            || std::env::var_os("RUVIC_FFMPEG_PATH").is_some_and(|v| !v.is_empty())
            || std::env::var_os("RUVIC_FFPROBE_PATH").is_some_and(|v| !v.is_empty())
    }
}

pub fn select_backend(
    config: &RuvicConfig,
    overrides: &BackendOverrides,
) -> RuviResult<Box<dyn MediaBackend>> {
    if overrides.has_explicit_ffmpeg(config) {
        let cli_ffmpeg = overrides
            .ffmpeg
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty());
        let cli_ffprobe = overrides
            .ffprobe
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty());
        let cfg_ffmpeg = config
            .backend
            .ffmpeg_path
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty());
        let cfg_ffprobe = config
            .backend
            .ffprobe_path
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty());
        let locations: FfmpegLocations =
            locate_ffmpeg(cli_ffmpeg, cli_ffprobe, cfg_ffmpeg, cfg_ffprobe)?;
        let timeout_secs = overrides
            .timeout_seconds
            .unwrap_or(config.backend.timeout_seconds);
        return Ok(Box::new(FfmpegBackend::new(
            locations,
            Duration::from_secs(timeout_secs),
        )));
    }

    Ok(Box::new(NativeMediaBackend::new()))
}
