use crate::domain::stream::{MediaStream, StreamKind};
use crate::error::{RuviError, RuviResult};

pub fn select_stream<'a>(streams: &'a [MediaStream], spec: &str) -> RuviResult<&'a MediaStream> {
    select_stream_with_lang(streams, spec, None)
}

/// Splits composite `primary|fallback` specs. Returns the effective spec for `streams`:
/// primary when it names an existing stream ID, otherwise fallback with the `:container` suffix removed.
/// Also returns the optional container preference.
fn resolve_composite_spec<'s>(
    streams: &[MediaStream],
    spec: &'s str,
) -> (&'s str, Option<&'s str>) {
    let trimmed = spec.trim();
    let Some((primary, fallback)) = trimmed.split_once('|') else {
        return split_container(trimmed);
    };
    let primary = primary.trim();
    if streams.iter().any(|s| s.id.eq_ignore_ascii_case(primary)) {
        return (primary, None);
    }
    split_container(fallback.trim())
}

fn split_container(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(':') {
        Some((base, cont))
            if cont.eq_ignore_ascii_case("mp4") || cont.eq_ignore_ascii_case("webm") =>
        {
            (base.trim(), Some(cont.trim()))
        }
        _ => (spec, None),
    }
}

pub fn select_stream_with_lang<'a>(
    streams: &'a [MediaStream],
    spec: &str,
    audio_lang: Option<&str>,
) -> RuviResult<&'a MediaStream> {
    let (spec, _container_pref) = resolve_composite_spec(streams, spec);
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
    if trimmed.eq_ignore_ascii_case("bestaudio")
        || trimmed.eq_ignore_ascii_case("audio")
        || trimmed.eq_ignore_ascii_case("m4a")
        || trimmed.eq_ignore_ascii_case("bestaudio[m4a]")
        || trimmed.eq_ignore_ascii_case("opus")
        || trimmed.eq_ignore_ascii_case("webm-audio")
    {
        let audios: Vec<&MediaStream> = streams
            .iter()
            .filter(|s| s.kind == StreamKind::Audio || s.kind == StreamKind::VideoAudio)
            .collect();
        let want_opus =
            trimmed.eq_ignore_ascii_case("opus") || trimmed.eq_ignore_ascii_case("webm-audio");
        // Prefer pure Audio streams first (especially m4a/mp4a for M4A compatibility),
        // then fall back to VideoAudio only if no pure Audio stream exists.
        let audio_only: Vec<&MediaStream> = audios
            .iter()
            .copied()
            .filter(|s| s.kind == StreamKind::Audio)
            .collect();
        let audio_pool = if audio_only.is_empty() {
            audios
        } else {
            audio_only
        };

        if want_opus {
            if let Some(best_opus) = audio_pool
                .iter()
                .copied()
                .filter(|s| {
                    s.container.eq_ignore_ascii_case("webm")
                        || s.codecs
                            .as_deref()
                            .is_some_and(|c| c.to_ascii_lowercase().contains("opus"))
                })
                .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
            {
                return Ok(best_opus);
            }
        }
        // Prefer m4a / mp4a / AAC audio streams first for optimal compatibility and direct playback, then highest bandwidth
        if let Some(best_m4a) = audio_pool
            .iter()
            .copied()
            .filter(|s| {
                s.container.eq_ignore_ascii_case("m4a") || s.container.eq_ignore_ascii_case("mp4")
            })
            .max_by_key(|s| s.bandwidth_bps.unwrap_or(0))
        {
            return Ok(best_m4a);
        }
        return audio_pool
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

    if trimmed.eq_ignore_ascii_case("h264") || trimmed.eq_ignore_ascii_case("mp4-h264") {
        if let Some(avc) = streams
            .iter()
            .filter(|s| s.kind == StreamKind::VideoAudio || s.kind == StreamKind::Video)
            .filter(|s| {
                s.codecs.as_deref().is_some_and(|c| {
                    let lower = c.to_ascii_lowercase();
                    lower.contains("avc") || lower.contains("h264")
                })
            })
            .max_by_key(|s| (s.height.unwrap_or(0), s.bandwidth_bps.unwrap_or(0)))
        {
            return Ok(avc);
        }
        return pool
            .into_iter()
            .max_by_key(|s| (s.height.unwrap_or(0), s.bandwidth_bps.unwrap_or(0)))
            .ok_or_else(|| RuviError::Unsupported("no stream available".to_string()));
    }

    // Match explicit ID first across all streams
    if let Some(exact) = streams.iter().find(|s| s.id.eq_ignore_ascii_case(trimmed)) {
        return Ok(exact);
    }

    // Match resolution selector like "1080p", "720p", "480p", "720" (prefer VideoAudio first, and MP4 over WebM at same height)
    let digits = trimmed.trim_end_matches(['p', 'P']);
    let mp4_pref_score = |s: &MediaStream| -> (u32, u64) {
        let is_mp4 = u32::from(s.container.eq_ignore_ascii_case("mp4"));
        (is_mp4, s.bandwidth_bps.unwrap_or(0))
    };
    if let Ok(target_height) = digits.parse::<u32>() {
        if let Some(exact_h) = pool
            .iter()
            .copied()
            .filter(|s| s.height == Some(target_height))
            .max_by_key(|s| mp4_pref_score(s))
        {
            return Ok(exact_h);
        }
        if let Some(exact_any) = streams
            .iter()
            .filter(|s| s.height == Some(target_height))
            .max_by_key(|s| mp4_pref_score(s))
        {
            return Ok(exact_any);
        }
        // Fallback to highest stream <= target height
        if let Some(le_h) = pool
            .iter()
            .copied()
            .filter(|s| s.height.is_some_and(|h| h <= target_height))
            .max_by_key(|s| (s.height.unwrap_or(0), mp4_pref_score(s)))
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

    let (trimmed, container_pref) = resolve_composite_spec(streams, spec);

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

    let video_score = |s: &MediaStream| -> (u32, u32, u32, u64) {
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
        let is_mp4 = u32::from(s.container.eq_ignore_ascii_case("mp4"));
        let format_pref = if container_pref.is_some() { 1 } else { is_mp4 };
        (h, fps_match, format_pref, s.bandwidth_bps.unwrap_or(0))
    };

    let selected_video: &MediaStream = if clean.is_empty() || clean == "best" {
        video_candidates
            .into_iter()
            .max_by_key(|s| video_score(s))?
    } else if clean == "h264" || clean == "mp4-h264" {
        video_candidates
            .iter()
            .copied()
            .filter(|s| {
                s.codecs.as_deref().is_some_and(|c| {
                    let lower = c.to_ascii_lowercase();
                    lower.contains("avc") || lower.contains("h264")
                })
            })
            .max_by_key(|s| video_score(s))
            .or_else(|| {
                video_candidates
                    .iter()
                    .copied()
                    .filter(|s| s.container.eq_ignore_ascii_case("mp4"))
                    .max_by_key(|s| video_score(s))
            })?
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
            // Find exact height match (preferring requested container if given)
            let exact_h = container_pref
                .and_then(|cont| {
                    video_candidates
                        .iter()
                        .copied()
                        .filter(|s| {
                            s.height == Some(target_h) && s.container.eq_ignore_ascii_case(cont)
                        })
                        .max_by_key(|s| video_score(s))
                })
                .or_else(|| {
                    video_candidates
                        .iter()
                        .copied()
                        .filter(|s| s.height == Some(target_h))
                        .max_by_key(|s| video_score(s))
                });

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

    let is_non_drc = |a: &MediaStream| -> u32 {
        match &a.payload {
            crate::domain::stream::StreamPayload::DirectHttp { url, .. } => {
                u32::from(!url.contains("xtags=drc"))
            }
            _ => 1,
        }
    };
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
        .max_by_key(|a| (is_non_drc(a), a.bandwidth_bps.unwrap_or(0)));

    compatible_audio.or_else(|| {
        audio_pool
            .into_iter()
            .max_by_key(|a| (is_non_drc(a), a.bandwidth_bps.unwrap_or(0)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::StreamPayload;

    fn mk_stream(
        id: &str,
        kind: StreamKind,
        container: &str,
        height: Option<u32>,
        fps: Option<&str>,
        bw: u64,
        codecs: Option<&str>,
    ) -> MediaStream {
        MediaStream {
            id: id.to_string(),
            kind,
            container: container.to_string(),
            width: height.map(|h| h * 16 / 9),
            height,
            bandwidth_bps: Some(bw),
            codecs: codecs.map(ToString::to_string),
            frame_rate: fps.map(ToString::to_string),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "https://example.com/test".to_string(),
                total_bytes: Some(1024),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        }
    }

    #[test]
    fn test_select_m4a_opus_2160p_and_1080p60() {
        let streams = vec![
            mk_stream(
                "itag-18",
                StreamKind::VideoAudio,
                "mp4",
                Some(360),
                Some("30"),
                400_000,
                Some("avc1"),
            ),
            mk_stream(
                "itag-137",
                StreamKind::Video,
                "mp4",
                Some(1080),
                Some("30"),
                3_000_000,
                Some("avc1"),
            ),
            mk_stream(
                "itag-299",
                StreamKind::Video,
                "mp4",
                Some(1080),
                Some("60"),
                5_000_000,
                Some("avc1"),
            ),
            mk_stream(
                "itag-313",
                StreamKind::Video,
                "webm",
                Some(2160),
                Some("24"),
                20_000_000,
                Some("vp09"),
            ),
            mk_stream(
                "itag-140",
                StreamKind::Audio,
                "m4a",
                None,
                None,
                128_000,
                Some("mp4a.40.2"),
            ),
            mk_stream(
                "itag-251",
                StreamKind::Audio,
                "webm",
                None,
                None,
                160_000,
                Some("opus"),
            ),
        ];

        assert_eq!(select_stream(&streams, "m4a").unwrap().id, "itag-140");
        assert_eq!(select_stream(&streams, "opus").unwrap().id, "itag-251");
        let (v60, a60) = select_stream_pair(&streams, "1080p60").unwrap();
        assert_eq!(v60.id, "itag-299");
        assert_eq!(a60.id, "itag-140");
        let (v4k, a4k) = select_stream_pair(&streams, "2160p").unwrap();
        assert_eq!(v4k.id, "itag-313");
        assert_eq!(a4k.id, "itag-251");

        // Composite fallback specs
        let (vc, _) = select_stream_pair(&streams, "itag-999|1080p60:mp4").unwrap();
        assert_eq!(vc.id, "itag-299");
        let (vw, aw) = select_stream_pair(&streams, "itag-998|2160p:webm").unwrap();
        assert_eq!(vw.id, "itag-313");
        assert_eq!(aw.id, "itag-251");
        let (vp, _) = select_stream_pair(&streams, "itag-137|2160p:webm").unwrap();
        assert_eq!(vp.id, "itag-137");
    }
}
