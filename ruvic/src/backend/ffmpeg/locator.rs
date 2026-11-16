use std::path::{Path, PathBuf};

use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone)]
pub struct FfmpegLocations {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredLocations {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

pub fn try_locate_ffmpeg(
    cli_ffmpeg: Option<&Path>,
    cli_ffprobe: Option<&Path>,
    cfg_ffmpeg: Option<&Path>,
    cfg_ffprobe: Option<&Path>,
) -> DiscoveredLocations {
    let env_ffmpeg = std::env::var_os("RUVIC_FFMPEG_PATH").map(PathBuf::from);
    let env_ffprobe = std::env::var_os("RUVIC_FFPROBE_PATH").map(PathBuf::from);

    let ffmpeg = resolve_binary("ffmpeg", cli_ffmpeg, env_ffmpeg.as_deref(), cfg_ffmpeg);
    let ffprobe = resolve_binary("ffprobe", cli_ffprobe, env_ffprobe.as_deref(), cfg_ffprobe);

    DiscoveredLocations { ffmpeg, ffprobe }
}

pub fn locate_ffmpeg(
    cli_ffmpeg: Option<&Path>,
    cli_ffprobe: Option<&Path>,
    cfg_ffmpeg: Option<&Path>,
    cfg_ffprobe: Option<&Path>,
) -> RuviResult<FfmpegLocations> {
    let discovered = try_locate_ffmpeg(cli_ffmpeg, cli_ffprobe, cfg_ffmpeg, cfg_ffprobe);
    let ffmpeg = discovered.ffmpeg.ok_or_else(|| {
        RuviError::Backend(
            "ffmpeg executable not found (install ffmpeg in PATH or pass --ffmpeg <PATH>)"
                .to_string(),
        )
    })?;
    let ffprobe = discovered.ffprobe.ok_or_else(|| {
        RuviError::Backend(
            "ffprobe executable not found (install ffprobe in PATH or pass --ffprobe <PATH>)"
                .to_string(),
        )
    })?;
    Ok(FfmpegLocations { ffmpeg, ffprobe })
}

fn resolve_binary(
    default_name: &str,
    cli_override: Option<&Path>,
    env_override: Option<&Path>,
    cfg_override: Option<&Path>,
) -> Option<PathBuf> {
    for explicit in [cli_override, env_override, cfg_override]
        .into_iter()
        .flatten()
    {
        if explicit.is_file() {
            return Some(explicit.to_path_buf());
        }
        if let Some(found) = search_in_path(explicit) {
            return Some(found);
        }
    }
    search_in_path(Path::new(default_name))
}

fn search_in_path(candidate: &Path) -> Option<PathBuf> {
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;
    let file_name = candidate.to_string_lossy();
    for dir in std::env::split_paths(&path_var) {
        let direct = dir.join(&*file_name);
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(windows)]
        {
            if candidate.extension().is_none() {
                for ext in ["exe", "cmd", "bat"] {
                    let with_ext = dir.join(format!("{file_name}.{ext}"));
                    if with_ext.is_file() {
                        return Some(with_ext);
                    }
                }
            }
        }
    }
    None
}
