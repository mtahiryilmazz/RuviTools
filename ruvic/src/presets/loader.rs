use std::path::Path;

use crate::domain::preset::Preset;
use crate::domain::stream::{AudioCodec, ContainerFormat, VideoCodec};
use crate::error::{RuviError, RuviResult};
use crate::planner::validation::{
    is_allowed_speed_preset, is_valid_bitrate, is_valid_pixel_format, validate_quality,
};
use crate::presets::builtin::find_builtin_preset;

pub fn load_preset_from_path(path: &Path) -> RuviResult<Preset> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        RuviError::Config(format!("failed to read preset {}: {err}", path.display()))
    })?;
    let preset: Preset = toml::from_str(&raw).map_err(|err| {
        RuviError::Config(format!("invalid TOML preset {}: {err}", path.display()))
    })?;
    validate_preset(&preset)?;
    Ok(preset)
}

pub fn find_preset(name_or_path: &str, user_presets_dir: Option<&Path>) -> RuviResult<Preset> {
    if let Some(builtin) = find_builtin_preset(name_or_path) {
        validate_preset(&builtin)?;
        return Ok(builtin);
    }

    let is_plain_name = !name_or_path.is_empty()
        && name_or_path != "."
        && name_or_path != ".."
        && !name_or_path.contains(['/', '\\']);
    if is_plain_name && let Some(dir) = user_presets_dir {
        let candidate = dir.join(format!("{name_or_path}.toml"));
        if candidate.is_file() {
            return load_preset_from_path(&candidate);
        }
    }

    let as_path = Path::new(name_or_path);
    if as_path.is_file() {
        return load_preset_from_path(as_path);
    }

    Err(RuviError::Config(format!(
        "preset not found: '{name_or_path}'"
    )))
}

pub fn validate_preset(preset: &Preset) -> RuviResult<()> {
    if preset.name.trim().is_empty() {
        return Err(RuviError::Config("preset name cannot be empty".to_string()));
    }
    let container = ContainerFormat::from_name(&preset.container.format)?;

    if let Some(ref video) = preset.video {
        let vcodec = VideoCodec::from_name(&video.codec)?;
        if !container.supports_video_codec(vcodec) {
            return Err(RuviError::Unsupported(format!(
                "preset '{}': container '{}' does not support video codec '{}'",
                preset.name, preset.container.format, video.codec
            )));
        }
        if let Some(ref speed) = video.preset
            && !is_allowed_speed_preset(speed)
        {
            return Err(RuviError::InvalidArgument(format!(
                "preset '{}': invalid encoder speed preset '{}'",
                preset.name, speed
            )));
        }
        if let Some(q) = video.quality {
            validate_quality(q, Some(vcodec)).map_err(|_| {
                RuviError::InvalidArgument(format!(
                    "preset '{}': quality {q} out of allowed range",
                    preset.name
                ))
            })?;
        }
        if let Some(ref pix_fmt) = video.pixel_format
            && !is_valid_pixel_format(pix_fmt)
        {
            return Err(RuviError::InvalidArgument(format!(
                "preset '{}': invalid pixel format '{pix_fmt}'",
                preset.name
            )));
        }
        if video.max_height == Some(0) {
            return Err(RuviError::InvalidArgument(format!(
                "preset '{}': max_height must be greater than 0",
                preset.name
            )));
        }
    }

    if let Some(ref audio) = preset.audio {
        let acodec = AudioCodec::from_name(&audio.codec)?;
        if !container.supports_audio_codec(acodec) {
            return Err(RuviError::Unsupported(format!(
                "preset '{}': container '{}' does not support audio codec '{}'",
                preset.name, preset.container.format, audio.codec
            )));
        }
        if let Some(ref bitrate) = audio.bitrate
            && !is_valid_bitrate(bitrate)
        {
            return Err(RuviError::InvalidArgument(format!(
                "preset '{}': invalid audio bitrate '{bitrate}'",
                preset.name
            )));
        }
    }

    if preset.video.is_none() && preset.audio.is_none() {
        return Err(RuviError::Config(format!(
            "preset '{}' must specify at least a video or audio configuration",
            preset.name
        )));
    }

    Ok(())
}
