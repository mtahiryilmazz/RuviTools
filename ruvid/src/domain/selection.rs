use crate::domain::stream::MediaStream;
use crate::error::{RuviError, RuviResult};

pub fn select_stream<'a>(streams: &'a [MediaStream], spec: &str) -> RuviResult<&'a MediaStream> {
    if streams.is_empty() {
        return Err(RuviError::Unsupported(
            "no downloadable streams found in media source".to_string(),
        ));
    }

    // For automatic selection (`best` / `worst`), prefer muxed VideoAudio streams when available
    // so downloads contain both video and audio without requiring external muxing.
    let combined: Vec<&MediaStream> = streams
        .iter()
        .filter(|s| s.kind == crate::domain::stream::StreamKind::VideoAudio)
        .collect();
    let pool: Vec<&MediaStream> = if combined.is_empty() {
        streams.iter().collect()
    } else {
        combined
    };

    let trimmed = spec.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("best") {
        return pool
            .into_iter()
            .max_by_key(|s| (s.height.unwrap_or(0), s.bandwidth_bps.unwrap_or(0)))
            .ok_or_else(|| RuviError::Unsupported("no stream available".to_string()));
    }

    if trimmed.eq_ignore_ascii_case("worst") {
        return pool
            .into_iter()
            .min_by_key(|s| {
                (
                    s.height.unwrap_or(u32::MAX),
                    s.bandwidth_bps.unwrap_or(u64::MAX),
                )
            })
            .ok_or_else(|| RuviError::Unsupported("no stream available".to_string()));
    }

    // Match explicit ID first across all streams
    if let Some(exact) = streams.iter().find(|s| s.id.eq_ignore_ascii_case(trimmed)) {
        return Ok(exact);
    }

    // Match resolution selector like "1080p", "720p", "480p", "720" (prefer VideoAudio first)
    let digits = trimmed.trim_end_matches(['p', 'P']);
    if let Ok(target_height) = digits.parse::<u32>() {
        if let Some(exact_h) = pool
            .iter()
            .copied()
            .filter(|s| s.height == Some(target_height))
            .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
        {
            return Ok(exact_h);
        }
        if let Some(exact_any) = streams
            .iter()
            .filter(|s| s.height == Some(target_height))
            .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
        {
            return Ok(exact_any);
        }
        // Fallback to highest stream <= target height
        if let Some(le_h) = pool
            .iter()
            .copied()
            .filter(|s| s.height.is_some_and(|h| h <= target_height))
            .max_by_key(|s| (s.height.unwrap_or(0), s.bandwidth_bps.unwrap_or(0)))
        {
            return Ok(le_h);
        }
    }

    Err(RuviError::InvalidArgument(format!(
        "no stream matching format selector '{spec}'"
    )))
}
