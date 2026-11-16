use std::path::{Component, Path, PathBuf};

use crate::error::{RuviError, RuviResult};

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
    "LPT8", "LPT9",
];

pub fn sanitize_filename(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }
    let cleaned = cleaned.replace("..", "_");
    let trimmed = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.');
    let candidate = if trimmed.is_empty() { "media" } else { trimmed };

    let stem_upper = candidate
        .split('.')
        .next()
        .unwrap_or(candidate)
        .trim()
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem_upper.as_str()) {
        format!("_{candidate}")
    } else {
        candidate.to_string()
    }
}

pub fn resolve_output_path(
    explicit_template: Option<&Path>,
    default_dir: &Path,
    title: &str,
    ext: &str,
    stream_id: &str,
) -> RuviResult<PathBuf> {
    let safe_title = sanitize_filename(title);
    let safe_ext = sanitize_filename(ext).trim_start_matches('.').to_string();
    let safe_ext = if safe_ext.is_empty() {
        "mp4".to_string()
    } else {
        safe_ext
    };
    let safe_id = sanitize_filename(stream_id);

    match explicit_template {
        Some(template) => {
            let raw = template.to_string_lossy();
            let expanded = raw
                .replace("%(title)s", &safe_title)
                .replace("%(ext)s", &safe_ext)
                .replace("%(id)s", &safe_id);
            let candidate = PathBuf::from(&expanded);

            // Prevent path traversal injected via placeholders (explicit relative `..` from user CLI is still checked if dangerous)
            if template.to_string_lossy().contains("%(")
                && candidate
                    .components()
                    .any(|c| matches!(c, Component::ParentDir))
            {
                return Err(RuviError::InvalidArgument(
                    "expanded output template must not contain '..' path traversal".to_string(),
                ));
            }

            if candidate.is_dir() || expanded.ends_with('/') || expanded.ends_with('\\') {
                Ok(candidate.join(format!("{safe_title}.{safe_ext}")))
            } else {
                Ok(candidate)
            }
        }
        None => Ok(default_dir.join(format!("{safe_title}.{safe_ext}"))),
    }
}
