use serde::Deserialize;

use crate::domain::media_info::{ChapterInfo, ContainerInfo, MediaInfo};
use crate::domain::stream::{StreamInfo, StreamKind};
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Deserialize)]
struct RawFfprobeOutput {
    format: Option<RawFormat>,
    #[serde(default)]
    streams: Vec<RawStream>,
    #[serde(default)]
    chapters: Vec<RawChapter>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    filename: Option<String>,
    format_name: Option<String>,
    format_long_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    index: Option<usize>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
    bit_rate: Option<String>,
    pix_fmt: Option<String>,
    tags: Option<RawTags>,
}

#[derive(Debug, Deserialize)]
struct RawChapter {
    id: Option<i64>,
    start_time: Option<String>,
    end_time: Option<String>,
    tags: Option<RawTags>,
}

#[derive(Debug, Deserialize)]
struct RawTags {
    language: Option<String>,
    title: Option<String>,
}

pub fn parse_ffprobe_json(json_str: &str, fallback_path: &str) -> RuviResult<MediaInfo> {
    let raw: RawFfprobeOutput = serde_json::from_str(json_str)
        .map_err(|err| RuviError::Backend(format!("failed to parse ffprobe JSON: {err}")))?;

    let fmt = raw.format.ok_or_else(|| {
        RuviError::Integrity("ffprobe output is missing container format metadata".to_string())
    })?;

    let format_name = fmt
        .format_name
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| RuviError::Integrity("ffprobe output is missing format_name".to_string()))?;

    let container = ContainerInfo {
        format_name,
        format_long_name: fmt.format_long_name,
        duration_seconds: fmt.duration.as_deref().and_then(parse_f64),
        size_bytes: fmt.size.as_deref().and_then(parse_u64),
        bit_rate: fmt.bit_rate.as_deref().and_then(parse_u64),
    };

    let streams = raw
        .streams
        .into_iter()
        .enumerate()
        .map(|(idx, s)| {
            let kind = match s.codec_type.as_deref() {
                Some("video") => StreamKind::Video,
                Some("audio") => StreamKind::Audio,
                Some("subtitle") => StreamKind::Subtitle,
                Some("data") => StreamKind::Data,
                _ => StreamKind::Unknown,
            };
            let fps = s
                .avg_frame_rate
                .as_deref()
                .and_then(parse_fraction)
                .or_else(|| s.r_frame_rate.as_deref().and_then(parse_fraction));
            StreamInfo {
                index: s.index.unwrap_or(idx),
                kind,
                codec_name: s.codec_name.unwrap_or_else(|| "unknown".to_string()),
                width: s.width,
                height: s.height,
                fps,
                sample_rate: s.sample_rate.as_deref().and_then(parse_u32),
                channels: s.channels,
                bit_rate: s.bit_rate.as_deref().and_then(parse_u64),
                language: s.tags.and_then(|t| t.language),
                pixel_format: s.pix_fmt,
                ..Default::default()
            }
        })
        .collect();

    let chapters = raw
        .chapters
        .into_iter()
        .enumerate()
        .map(|(idx, c)| ChapterInfo {
            id: c.id.unwrap_or(idx as i64),
            start_seconds: c.start_time.as_deref().and_then(parse_f64).unwrap_or(0.0),
            end_seconds: c.end_time.as_deref().and_then(parse_f64).unwrap_or(0.0),
            title: c.tags.and_then(|t| t.title),
        })
        .collect();

    Ok(MediaInfo {
        path: fmt.filename.unwrap_or_else(|| fallback_path.to_string()),
        container,
        streams,
        chapters,
    })
}

fn parse_f64(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn parse_u64(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
}

fn parse_u32(raw: &str) -> Option<u32> {
    raw.trim().parse::<u32>().ok()
}

fn parse_fraction(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if let Some((num, den)) = trimmed.split_once('/') {
        let n = num.parse::<f64>().ok()?;
        let d = den.parse::<f64>().ok()?;
        if d > 0.0 && n >= 0.0 {
            let val = n / d;
            if val.is_finite() && val > 0.0 {
                return Some((val * 1000.0).round() / 1000.0);
            }
        }
        None
    } else {
        parse_f64(trimmed).filter(|v| *v > 0.0)
    }
}
