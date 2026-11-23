use std::path::{Component, Path, PathBuf};

use crate::domain::media_info::MediaInfo;
use crate::domain::output_spec::OverwritePolicy;
use crate::domain::stream::{AudioCodec, ContainerFormat, StreamKind, VideoCodec};
use crate::error::{RuviError, RuviResult};

pub fn validate_io_paths(
    input: &Path,
    output: &Path,
    overwrite: OverwritePolicy,
) -> RuviResult<()> {
    if !input.exists() {
        return Err(RuviError::Filesystem(format!(
            "input file not found: {}",
            input.display()
        )));
    }
    if !input.is_file() {
        return Err(RuviError::InvalidArgument(format!(
            "input path is not a file: {}",
            input.display()
        )));
    }

    if output.is_dir() {
        return Err(RuviError::InvalidArgument(format!(
            "output path is a directory: {}",
            output.display()
        )));
    }

    if paths_refer_to_same_file(input, output)? {
        return Err(RuviError::InvalidArgument(format!(
            "input and output refer to the same file: {}",
            output.display()
        )));
    }

    let output_exists = output.exists() || std::fs::symlink_metadata(output).is_ok();
    if output_exists && overwrite == OverwritePolicy::Reject {
        return Err(RuviError::Filesystem(format!(
            "output file already exists: {} (use --overwrite to replace)",
            output.display()
        )));
    }

    Ok(())
}

pub fn paths_refer_to_same_file(a: &Path, b: &Path) -> RuviResult<bool> {
    let canon_a = canonicalize_best_effort(a)?;
    let canon_b = canonicalize_best_effort(b)?;

    #[cfg(windows)]
    {
        Ok(canon_a
            .to_string_lossy()
            .eq_ignore_ascii_case(&canon_b.to_string_lossy()))
    }
    #[cfg(not(windows))]
    {
        Ok(canon_a == canon_b)
    }
}

fn canonicalize_best_effort(path: &Path) -> RuviResult<PathBuf> {
    if std::fs::symlink_metadata(path).is_ok()
        && let Ok(canon) = std::fs::canonicalize(path)
    {
        return Ok(canon);
    }

    let abs = if path.is_relative() {
        std::env::current_dir()?.join(path)
    } else {
        path.to_path_buf()
    };

    let mut resolved = PathBuf::new();
    for component in abs.components() {
        match component {
            Component::Prefix(p) => resolved.push(p.as_os_str()),
            Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(name) => resolved.push(name),
            Component::ParentDir => {
                if std::fs::symlink_metadata(&resolved).is_ok()
                    && let Ok(canon) = std::fs::canonicalize(&resolved)
                {
                    resolved = canon;
                }
                if matches!(
                    resolved.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    resolved.pop();
                }
            }
        }
    }

    if std::fs::symlink_metadata(&resolved).is_ok()
        && let Ok(canon) = std::fs::canonicalize(&resolved)
    {
        return Ok(canon);
    }

    for ancestor in resolved.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor).is_ok()
            && let Ok(canon_anc) = std::fs::canonicalize(ancestor)
            && let Ok(tail) = resolved.strip_prefix(ancestor)
        {
            return Ok(if tail.as_os_str().is_empty() {
                canon_anc
            } else {
                canon_anc.join(tail)
            });
        }
    }

    Ok(resolved)
}

pub fn validate_selected_streams(
    info: &MediaInfo,
    video_stream: Option<usize>,
    audio_stream: Option<usize>,
) -> RuviResult<()> {
    if let Some(idx) = video_stream {
        let valid = info
            .streams
            .iter()
            .any(|s| s.index == idx && s.kind == StreamKind::Video);
        if !valid {
            return Err(RuviError::InvalidArgument(format!(
                "requested video stream index {idx} not found in input media"
            )));
        }
    }
    if let Some(idx) = audio_stream {
        let valid = info
            .streams
            .iter()
            .any(|s| s.index == idx && s.kind == StreamKind::Audio);
        if !valid {
            return Err(RuviError::InvalidArgument(format!(
                "requested audio stream index {idx} not found in input media"
            )));
        }
    }
    Ok(())
}

fn validate_stream_copy_compatibility(
    info: &MediaInfo,
    target: ContainerFormat,
    kind: StreamKind,
    selected_stream: Option<usize>,
    context: &str,
) -> RuviResult<()> {
    for stream in &info.streams {
        if stream.kind != kind {
            continue;
        }
        if let Some(sel) = selected_stream
            && stream.index != sel
        {
            continue;
        }
        let (kind_label, supported) = match kind {
            StreamKind::Video => (
                "video",
                target.supports_raw_video_codec_name(&stream.codec_name),
            ),
            StreamKind::Audio => (
                "audio",
                target.supports_raw_audio_codec_name(&stream.codec_name),
            ),
            _ => continue,
        };
        if !supported {
            return Err(RuviError::Unsupported(format!(
                "container '{}' is not compatible with {kind_label} codec '{}' during {context} (transcoding required)",
                target.as_str(),
                stream.codec_name
            )));
        }
    }
    Ok(())
}

pub fn validate_video_copy_compatibility(
    info: &MediaInfo,
    target: ContainerFormat,
    video_stream: Option<usize>,
) -> RuviResult<()> {
    if target.is_audio_only() || target.is_image() {
        return Err(RuviError::Unsupported(format!(
            "container '{}' does not support video streams",
            target.as_str()
        )));
    }
    validate_stream_copy_compatibility(info, target, StreamKind::Video, video_stream, "copy")
}

pub fn validate_audio_copy_compatibility(
    info: &MediaInfo,
    target: ContainerFormat,
    audio_stream: Option<usize>,
) -> RuviResult<()> {
    if target.is_image() {
        return Err(RuviError::Unsupported(format!(
            "container '{}' does not support audio streams",
            target.as_str()
        )));
    }
    validate_stream_copy_compatibility(info, target, StreamKind::Audio, audio_stream, "copy")
}

pub fn is_allowed_speed_preset(speed: &str) -> bool {
    matches!(
        speed.trim().to_ascii_lowercase().as_str(),
        "ultrafast"
            | "superfast"
            | "veryfast"
            | "faster"
            | "fast"
            | "medium"
            | "slow"
            | "slower"
            | "veryslow"
    )
}

pub fn is_valid_bitrate(rate: &str) -> bool {
    let trimmed = rate.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.eq_ignore_ascii_case("vbr") || trimmed == "0" {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    let digits = lower.trim_end_matches("bps").trim_end_matches(['k', 'm']);
    !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && digits.parse::<u64>().is_ok_and(|v| v > 0)
}

pub fn is_valid_pixel_format(pix_fmt: &str) -> bool {
    let trimmed = pix_fmt.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn validate_quality(quality: u8, video_codec: Option<VideoCodec>) -> RuviResult<()> {
    let max_q = match video_codec {
        Some(VideoCodec::H264 | VideoCodec::H265) => 51,
        _ => 63,
    };
    if quality > max_q {
        return Err(RuviError::InvalidArgument(format!(
            "quality {quality} out of allowed range (0..={max_q})"
        )));
    }
    Ok(())
}

pub fn validate_timestamp(raw: &str, flag: &str) -> RuviResult<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed != raw {
        return Err(RuviError::InvalidArgument(format!(
            "{flag} timestamp must be non-empty without surrounding whitespace"
        )));
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return Err(RuviError::InvalidArgument(format!(
            "invalid {flag} timestamp '{raw}' (expected seconds or HH:MM:SS)"
        )));
    }
    for (idx, part) in parts.iter().enumerate() {
        let is_last = idx + 1 == parts.len();
        let valid = if is_last {
            if let Some((whole, frac)) = part.split_once('.') {
                !whole.is_empty()
                    && !frac.is_empty()
                    && whole.chars().all(|c| c.is_ascii_digit())
                    && frac.chars().all(|c| c.is_ascii_digit())
            } else {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())
            }
        } else {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())
        };
        if !valid {
            return Err(RuviError::InvalidArgument(format!(
                "invalid {flag} timestamp '{raw}' (expected seconds or HH:MM:SS)"
            )));
        }
    }
    Ok(())
}

pub fn parse_timestamp_seconds(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if let Ok(sec) = trimmed.parse::<f64>() {
        return sec.is_finite().then_some(sec.max(0.0));
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.as_slice() {
        [mm, ss] => {
            let m = mm.parse::<f64>().ok()?;
            let s = ss.parse::<f64>().ok()?;
            let total = m * 60.0 + s;
            total.is_finite().then_some(total.max(0.0))
        }
        [hh, mm, ss] => {
            let h = hh.parse::<f64>().ok()?;
            let m = mm.parse::<f64>().ok()?;
            let s = ss.parse::<f64>().ok()?;
            let total = h * 3600.0 + m * 60.0 + s;
            total.is_finite().then_some(total.max(0.0))
        }
        _ => None,
    }
}

pub fn validate_remux_compatibility(
    info: &MediaInfo,
    target: ContainerFormat,
    video_stream: Option<usize>,
    audio_stream: Option<usize>,
) -> RuviResult<()> {
    if target.is_audio_only() || target.is_image() {
        return Err(RuviError::Unsupported(format!(
            "remux cannot target '{}' (use `ruvic audio` or `ruvic thumbnail` instead)",
            target.as_str()
        )));
    }

    validate_stream_copy_compatibility(info, target, StreamKind::Video, video_stream, "remux")?;
    validate_stream_copy_compatibility(info, target, StreamKind::Audio, audio_stream, "remux")?;

    Ok(())
}

pub fn validate_target_codecs(
    container: ContainerFormat,
    video: Option<VideoCodec>,
    audio: Option<AudioCodec>,
) -> RuviResult<()> {
    if let Some(v) = video
        && !container.supports_video_codec(v)
    {
        return Err(RuviError::Unsupported(format!(
            "container '{}' does not support video codec '{}'",
            container.as_str(),
            v.default_ffmpeg_encoder()
        )));
    }
    if let Some(a) = audio
        && !container.supports_audio_codec(a)
    {
        return Err(RuviError::Unsupported(format!(
            "container '{}' does not support audio codec '{}'",
            container.as_str(),
            a.default_ffmpeg_encoder()
        )));
    }
    Ok(())
}
