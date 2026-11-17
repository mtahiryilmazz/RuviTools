use crate::domain::stream::{MediaStream, StreamKind};
use crate::error::{RuviError, RuviResult};

pub fn select_stream<'a>(streams: &'a [MediaStream], spec: &str) -> RuviResult<&'a MediaStream> {
    select_stream_with_lang(streams, spec, None)
}

pub fn select_stream_with_lang<'a>(
    streams: &'a [MediaStream],
    spec: &str,
    audio_lang: Option<&str>,
) -> RuviResult<&'a MediaStream> {
    if streams.is_empty() {
        return Err(RuviError::Unsupported(
            "no downloadable streams found in media source".to_string(),
        ));
    }

    if let Some(lang) = audio_lang {
        let lang_audio: Vec<&MediaStream> = streams
            .iter()
            .filter(|s| {
                (s.kind == StreamKind::Audio || s.kind == StreamKind::VideoAudio)
                    && s.language
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case(lang) || l.starts_with(lang))
            })
            .collect();
        if !lang_audio.is_empty() {
            return lang_audio
                .into_iter()
                .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
                .ok_or_else(|| RuviError::Unsupported("no stream available".to_string()));
        }
    }

    let trimmed = spec.trim();
    if trimmed.eq_ignore_ascii_case("bestaudio") || trimmed.eq_ignore_ascii_case("audio") {
        let audios: Vec<&MediaStream> = streams
            .iter()
            .filter(|s| s.kind == StreamKind::Audio || s.kind == StreamKind::VideoAudio)
            .collect();
        // Prefer m4a / mp4a / AAC audio streams first for optimal compatibility and direct playback, then highest bandwidth
        if let Some(best_m4a) = audios
            .iter()
            .copied()
            .filter(|s| {
                s.container.eq_ignore_ascii_case("m4a") || s.container.eq_ignore_ascii_case("mp4")
            })
            .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
        {
            return Ok(best_m4a);
        }
        return audios
            .into_iter()
            .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
            .ok_or_else(|| RuviError::Unsupported("no audio stream available".to_string()));
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

/// Selects a separate video and audio stream pair for DASH / adaptive media.
///
/// Returns:
/// - `Some((video, audio))` if an adaptive video stream matches the spec and has a compatible audio stream.
/// - `None` if the matching stream is already muxed `VideoAudio` (preserving single-stream downloading),
///   or if no audio stream exists.
pub fn select_stream_pair<'a>(
    streams: &'a [MediaStream],
    spec: &str,
) -> Option<(&'a MediaStream, &'a MediaStream)> {
    select_stream_pair_with_lang(streams, spec, None)
}

pub fn select_stream_pair_with_lang<'a>(
    streams: &'a [MediaStream],
    spec: &str,
    audio_lang: Option<&str>,
) -> Option<(&'a MediaStream, &'a MediaStream)> {
    if streams.is_empty() {
        return None;
    }

    let trimmed = spec.trim();

    // 1. Check if user specified an explicit stream ID
    if let Some(exact) = streams.iter().find(|s| s.id.eq_ignore_ascii_case(trimmed)) {
        if exact.kind == StreamKind::VideoAudio {
            return None;
        }
        if exact.kind == StreamKind::Video {
            let audio = find_best_audio_with_lang(streams, exact, audio_lang)?;
            return Some((exact, audio));
        }
        return None;
    }

    // 2. "worst" selection is handled by single-stream
    if trimmed.eq_ignore_ascii_case("worst") {
        return None;
    }

    // 3. Collect video candidates
    let video_candidates: Vec<&MediaStream> = streams
        .iter()
        .filter(|s| s.kind == StreamKind::Video || s.kind == StreamKind::VideoAudio)
        .collect();

    if video_candidates.is_empty() {
        return None;
    }

    let clean = trimmed.to_ascii_lowercase();
    let prefer_60 = clean.contains("60");

    let video_score = |s: &MediaStream| -> (u32, u32, u64) {
        let h = s.height.unwrap_or(0);
        let fps = s
            .frame_rate
            .as_deref()
            .and_then(|f| f.parse::<u32>().ok())
            .unwrap_or(30);
        let fps_match = if prefer_60 {
            if fps >= 60 { 2 } else { 1 }
        } else {
            1
        };
        (h, fps_match, s.bandwidth_bps.unwrap_or(0))
    };

    let selected_video: &MediaStream = if clean.is_empty() || clean == "best" {
        video_candidates
            .into_iter()
            .max_by_key(|s| video_score(s))?
    } else {
        let target_height = if clean.starts_with("4k") || clean.starts_with("uhd") {
            Some(2160)
        } else if clean.starts_with("8k") {
            Some(4320)
        } else {
            let digits: String = clean.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok()
        };

        let target_h = target_height?;
        {
            // Find exact height match
            let exact_h = video_candidates
                .iter()
                .copied()
                .filter(|s| s.height == Some(target_h))
                .max_by_key(|s| video_score(s));

            if let Some(v) = exact_h {
                v
            } else {
                // Fallback to highest <= target_height
                let le_h = video_candidates
                    .iter()
                    .copied()
                    .filter(|s| s.height.is_some_and(|h| h <= target_h))
                    .max_by_key(|s| video_score(s));
                if let Some(v) = le_h {
                    v
                } else {
                    video_candidates
                        .into_iter()
                        .max_by_key(|s| video_score(s))?
                }
            }
        }
    };

    // If the selected stream is already muxed VideoAudio, do not dual-stream
    if selected_video.kind == StreamKind::VideoAudio {
        return None;
    }
    if selected_video.kind != StreamKind::Video {
        return None;
    }

    let best_audio = find_best_audio_with_lang(streams, selected_video, audio_lang)?;
    Some((selected_video, best_audio))
}

fn find_best_audio_with_lang<'a>(
    streams: &'a [MediaStream],
    video: &MediaStream,
    audio_lang: Option<&str>,
) -> Option<&'a MediaStream> {
    let all_audio: Vec<&MediaStream> = streams
        .iter()
        .filter(|s| s.kind == StreamKind::Audio)
        .collect();

    if all_audio.is_empty() {
        return None;
    }

    let lang_filtered: Vec<&MediaStream> = if let Some(lang) = audio_lang {
        let matched: Vec<&MediaStream> = all_audio
            .iter()
            .copied()
            .filter(|a| {
                a.language
                    .as_deref()
                    .is_some_and(|l| l.eq_ignore_ascii_case(lang) || l.starts_with(lang))
            })
            .collect();
        if matched.is_empty() {
            all_audio
        } else {
            matched
        }
    } else {
        all_audio
    };

    let audio_pool = lang_filtered;

    let is_webm = video.container.eq_ignore_ascii_case("webm");
    let compatible_audio = audio_pool
        .iter()
        .copied()
        .filter(|a| {
            if is_webm {
                a.container.eq_ignore_ascii_case("webm")
                    || a.codecs
                        .as_deref()
                        .is_some_and(|c| c.to_ascii_lowercase().contains("opus"))
            } else {
                a.container.eq_ignore_ascii_case("m4a")
                    || a.container.eq_ignore_ascii_case("mp4")
                    || a.codecs
                        .as_deref()
                        .is_some_and(|c| c.to_ascii_lowercase().contains("mp4a"))
            }
        })
        .max_by_key(|a| a.bandwidth_bps.unwrap_or(0));

    compatible_audio.or_else(|| {
        audio_pool
            .into_iter()
            .max_by_key(|a| a.bandwidth_bps.unwrap_or(0))
    })
}
