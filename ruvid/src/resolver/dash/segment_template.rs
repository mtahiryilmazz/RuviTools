use url::Url;

use crate::domain::stream::MediaSegment;
use crate::error::{RuviError, RuviResult};
use crate::resolver::MAX_PLAYLIST_SEGMENTS;
use crate::resolver::dash::mpd::SegmentTemplateSpec;

pub fn generate_segments_from_template(
    tpl: &SegmentTemplateSpec,
    base_url: &Url,
    rep_id: &str,
    bandwidth: Option<u64>,
    period_duration_seconds: Option<f64>,
) -> RuviResult<Vec<MediaSegment>> {
    let media_pattern = tpl.media.as_deref().ok_or_else(|| {
        RuviError::Unsupported("DASH SegmentTemplate is missing 'media' attribute".to_string())
    })?;
    let start_number = tpl.start_number.unwrap_or(1);
    let timescale = tpl.timescale.unwrap_or(1).max(1);

    let mut segments = Vec::new();

    if !tpl.timeline.is_empty() {
        let mut current_time = 0u64;
        let mut current_number = start_number;
        for entry in &tpl.timeline {
            if let Some(t) = entry.start_time {
                current_time = t;
            }
            if entry.repeat < 0 {
                return Err(RuviError::Unsupported(
                    "unbounded SegmentTimeline repeat (-1) is unsupported in VOD mode".to_string(),
                ));
            }
            let count = (entry.repeat as u64).saturating_add(1);
            for _ in 0..count {
                if segments.len() >= MAX_PLAYLIST_SEGMENTS {
                    return Err(RuviError::Unsupported(format!(
                        "DASH segment count exceeds limit ({MAX_PLAYLIST_SEGMENTS})"
                    )));
                }
                let rel = expand_template(
                    media_pattern,
                    rep_id,
                    bandwidth,
                    current_number,
                    current_time,
                );
                let url = base_url
                    .join(&rel)
                    .map_err(|err| RuviError::Unsupported(err.to_string()))?;
                segments.push(MediaSegment {
                    index: segments.len(),
                    url: url.to_string(),
                    byte_range: None,
                    duration_seconds: Some((entry.duration / timescale).max(1)),
                });
                current_time = current_time.saturating_add(entry.duration);
                current_number = current_number.saturating_add(1);
            }
        }
        return Ok(segments);
    }

    let seg_dur = tpl.duration.ok_or_else(|| {
        RuviError::Unsupported(
            "DASH SegmentTemplate requires either SegmentTimeline or duration attribute"
                .to_string(),
        )
    })?;
    let total_secs = period_duration_seconds.ok_or_else(|| {
        RuviError::Unsupported(
            "DASH MPD requires mediaPresentationDuration or Period duration when using fixed SegmentTemplate"
                .to_string(),
        )
    })?;
    let seg_secs = (seg_dur as f64) / (timescale as f64);
    if seg_secs <= 0.0 {
        return Err(RuviError::Unsupported(
            "invalid DASH segment duration <= 0".to_string(),
        ));
    }
    let count = (total_secs / seg_secs).ceil().max(1.0) as usize;
    if count > MAX_PLAYLIST_SEGMENTS {
        return Err(RuviError::Unsupported(format!(
            "DASH segment count ({count}) exceeds limit ({MAX_PLAYLIST_SEGMENTS})"
        )));
    }

    for i in 0..count {
        let num = start_number.saturating_add(i as u64);
        let time = (i as u64).saturating_mul(seg_dur);
        let rel = expand_template(media_pattern, rep_id, bandwidth, num, time);
        let url = base_url
            .join(&rel)
            .map_err(|err| RuviError::Unsupported(err.to_string()))?;
        segments.push(MediaSegment {
            index: i,
            url: url.to_string(),
            byte_range: None,
            duration_seconds: Some(seg_secs.round() as u64),
        });
    }

    Ok(segments)
}

pub fn expand_template(
    pattern: &str,
    rep_id: &str,
    bandwidth: Option<u64>,
    number: u64,
    time: u64,
) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut rest = pattern;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('$') else {
            out.push('$');
            out.push_str(after);
            return out;
        };
        let token = &after[..end];
        if token.is_empty() {
            out.push('$');
        } else if token == "RepresentationID" {
            out.push_str(rep_id);
        } else if let Some(val) = format_numeric_token(token, "Number", number) {
            out.push_str(&val);
        } else if let Some(val) = format_numeric_token(token, "Time", time) {
            out.push_str(&val);
        } else if let Some(val) = format_numeric_token(token, "Bandwidth", bandwidth.unwrap_or(0)) {
            out.push_str(&val);
        } else {
            out.push('$');
            out.push_str(token);
            out.push('$');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn format_numeric_token(token: &str, name: &str, value: u64) -> Option<String> {
    if token == name {
        return Some(value.to_string());
    }
    let suffix = token.strip_prefix(name)?;
    if let Some(fmt) = suffix.strip_prefix('%')
        && let Some(width_str) = fmt.strip_suffix('d')
        && let Ok(width) = width_str.trim_start_matches('0').parse::<usize>()
    {
        return Some(format!("{value:0width$}"));
    }
    None
}
