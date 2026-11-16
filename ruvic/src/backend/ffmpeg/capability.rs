use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::backend::BackendCapabilities;
use crate::error::{RuviError, RuviResult};

const CAPABILITY_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn probe_capabilities(ffmpeg: &Path, ffprobe: &Path) -> RuviResult<BackendCapabilities> {
    let ffmpeg_ver = run_version_cmd(ffmpeg).await?;
    let ffprobe_ver = run_version_cmd(ffprobe).await?;
    let encoders_raw = run_simple_query(ffmpeg, &["-hide_banner", "-encoders"]).await?;
    let hwaccels_raw = run_simple_query(ffmpeg, &["-hide_banner", "-hwaccels"])
        .await
        .unwrap_or_default();

    Ok(BackendCapabilities {
        ffmpeg_version: ffmpeg_ver,
        ffprobe_version: ffprobe_ver,
        encoders: parse_encoders_output(&encoders_raw),
        hwaccels: parse_hwaccels_output(&hwaccels_raw),
    })
}

async fn run_version_cmd(bin: &Path) -> RuviResult<Option<String>> {
    let out = run_simple_query(bin, &["-version"]).await?;
    Ok(out.lines().next().map(|s| s.trim().to_string()))
}

async fn run_simple_query(bin: &Path, args: &[&str]) -> RuviResult<String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = tokio::time::timeout(CAPABILITY_PROBE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            RuviError::Backend(format!(
                "timed out querying capability from {}",
                bin.display()
            ))
        })?
        .map_err(|err| RuviError::Backend(format!("failed to invoke {}: {err}", bin.display())))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RuviError::Backend(format!(
            "{} exited with {}: {}",
            bin.display(),
            output.status,
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn parse_encoders_output(output: &str) -> Vec<String> {
    let mut encoders = Vec::new();
    let mut in_list = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "------" {
            in_list = true;
            continue;
        }
        if !in_list && trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(flags) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        if flags.len() == 6
            && (flags.starts_with('V') || flags.starts_with('A') || flags.starts_with('S'))
        {
            encoders.push(name.to_string());
        }
    }
    encoders
}

pub fn parse_hwaccels_output(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.eq_ignore_ascii_case("Hardware acceleration methods:")
        })
        .map(ToString::to_string)
        .collect()
}
