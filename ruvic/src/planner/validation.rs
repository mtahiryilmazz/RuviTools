use std::path::Path;

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

    if paths_refer_to_same_file(input, output)? {
        return Err(RuviError::InvalidArgument(format!(
            "input and output refer to the same file: {}",
            output.display()
        )));
    }

    if output.exists() && overwrite == OverwritePolicy::Reject {
        return Err(RuviError::Filesystem(format!(
            "output file already exists: {} (use --overwrite to replace)",
            output.display()
        )));
    }

    Ok(())
}

pub fn paths_refer_to_same_file(a: &Path, b: &Path) -> RuviResult<bool> {
    let canon_a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let canon_b = if b.exists() {
        std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf())
    } else if let Some(parent) = b.parent().filter(|p| !p.as_os_str().is_empty()) {
        let canon_parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
        match b.file_name() {
            Some(name) => canon_parent.join(name),
            None => canon_parent,
        }
    } else {
        let cwd = std::env::current_dir()?;
        let canon_cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
        match b.file_name() {
            Some(name) => canon_cwd.join(name),
            None => canon_cwd,
        }
    };

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

    for stream in &info.streams {
        match stream.kind {
            StreamKind::Video => {
                if let Some(sel) = video_stream
                    && stream.index != sel
                {
                    continue;
                }
                if !target.supports_raw_video_codec_name(&stream.codec_name) {
                    return Err(RuviError::Unsupported(format!(
                        "container '{}' is not compatible with video codec '{}' during remux (transcoding required)",
                        target.as_str(),
                        stream.codec_name
                    )));
                }
            }
            StreamKind::Audio => {
                if let Some(sel) = audio_stream
                    && stream.index != sel
                {
                    continue;
                }
                if !target.supports_raw_audio_codec_name(&stream.codec_name) {
                    return Err(RuviError::Unsupported(format!(
                        "container '{}' is not compatible with audio codec '{}' during remux (transcoding required)",
                        target.as_str(),
                        stream.codec_name
                    )));
                }
            }
            _ => {}
        }
    }

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
