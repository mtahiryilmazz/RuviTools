pub mod mpd;
pub mod parser;
pub mod segment_template;

use url::Url;

use crate::domain::media_source::{MediaSource, SourceKind, SubtitleTrack};
use crate::domain::stream::{MediaSegment, MediaStream, StreamKind, StreamPayload};
use crate::error::{RuviError, RuviResult};
use crate::resolver::dash::mpd::{MpdDocument, SegmentTemplateSpec};
use crate::resolver::dash::parser::parse_mpd_xml;
use crate::resolver::dash::segment_template::{expand_template, generate_segments_from_template};

pub fn resolve_dash_from_xml(
    original_url: &str,
    mpd_url: &Url,
    xml_body: &str,
) -> RuviResult<MediaSource> {
    let doc: MpdDocument = parse_mpd_xml(xml_body)?;
    if doc.mpd_type.eq_ignore_ascii_case("dynamic") {
        return Err(RuviError::Unsupported(
            "dynamic/live DASH MPDs are not supported in VOD mode".to_string(),
        ));
    }

    let mut streams = Vec::new();
    let mut subtitles = Vec::new();

    for period in &doc.periods {
        let period_duration = period.duration_seconds.or(doc.media_duration_seconds);
        let period_base = join_base_url(mpd_url, doc.base_url.as_deref())?;
        let period_base = join_base_url(&period_base, period.base_url.as_deref())?;

        for aset in &period.adaptation_sets {
            let aset_base = join_base_url(&period_base, aset.base_url.as_deref())?;
            let is_text = aset
                .content_type
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case("text"))
                || aset
                    .mime_type
                    .as_deref()
                    .is_some_and(|m| m.contains("vtt") || m.contains("subrip"));

            if is_text {
                for rep in &aset.representations {
                    let rep_base = join_base_url(&aset_base, rep.base_url.as_deref())?;
                    let format = if aset
                        .mime_type
                        .as_deref()
                        .or(rep.mime_type.as_deref())
                        .is_some_and(|m| m.contains("srt"))
                    {
                        "srt"
                    } else {
                        "vtt"
                    };
                    subtitles.push(SubtitleTrack {
                        language: aset.lang.clone().unwrap_or_else(|| "und".to_string()),
                        name: Some(rep.id.clone()),
                        url: rep_base.to_string(),
                        format: format.to_string(),
                    });
                }
                continue;
            }

            let kind =
                determine_stream_kind(aset.content_type.as_deref(), aset.mime_type.as_deref());

            for rep in &aset.representations {
                let rep_base = join_base_url(&aset_base, rep.base_url.as_deref())?;
                let effective_tpl = merge_templates(
                    aset.segment_template.as_ref(),
                    rep.segment_template.as_ref(),
                );

                let (init_url, segments) = if let Some(ref tpl) = effective_tpl {
                    let init = tpl
                        .initialization
                        .as_deref()
                        .map(|pat| {
                            let expanded = expand_template(pat, &rep.id, rep.bandwidth, 0, 0);
                            rep_base
                                .join(&expanded)
                                .map(|u| u.to_string())
                                .map_err(|err| RuviError::Unsupported(err.to_string()))
                        })
                        .transpose()?;
                    let segs = generate_segments_from_template(
                        tpl,
                        &rep_base,
                        &rep.id,
                        rep.bandwidth,
                        period_duration,
                    )?;
                    (init, segs)
                } else if let Some(ref list) = rep.segment_list {
                    let init = list
                        .initialization
                        .as_deref()
                        .map(|u| {
                            rep_base
                                .join(u)
                                .map(|r| r.to_string())
                                .map_err(|err| RuviError::Unsupported(err.to_string()))
                        })
                        .transpose()?;
                    let mut segs = Vec::with_capacity(list.segments.len());
                    for (idx, seg_rel) in list.segments.iter().enumerate() {
                        let seg_url = rep_base
                            .join(seg_rel)
                            .map_err(|err| RuviError::Unsupported(err.to_string()))?;
                        segs.push(MediaSegment {
                            index: idx,
                            url: seg_url.to_string(),
                            byte_range: None,
                            duration_seconds: None,
                        });
                    }
                    (init, segs)
                } else if rep.base_url.is_some() || aset.base_url.is_some() {
                    // Single-file DASH representation via BaseURL
                    let segs = vec![MediaSegment {
                        index: 0,
                        url: rep_base.to_string(),
                        byte_range: None,
                        duration_seconds: period_duration.map(|d| d.round() as u64),
                    }];
                    (None, segs)
                } else {
                    return Err(RuviError::Unsupported(format!(
                        "DASH representation '{}' has neither SegmentTemplate, SegmentList, nor BaseURL",
                        rep.id
                    )));
                };

                let mime = rep
                    .mime_type
                    .as_deref()
                    .or(aset.mime_type.as_deref())
                    .unwrap_or("video/mp4");
                let container = if mime.contains("webm") {
                    "webm"
                } else if mime.contains("audio/mp4") {
                    "m4a"
                } else {
                    "mp4"
                };

                streams.push(MediaStream {
                    id: rep.id.clone(),
                    kind,
                    container: container.to_string(),
                    width: rep.width,
                    height: rep.height,
                    bandwidth_bps: rep.bandwidth,
                    codecs: rep.codecs.clone(),
                    frame_rate: rep.frame_rate.clone(),
                    language: aset.lang.clone(),
                    payload: StreamPayload::Dash {
                        mpd_url: mpd_url.to_string(),
                        init_segment_url: init_url,
                        segments,
                    },
                });
            }
        }
    }

    if streams.is_empty() {
        return Err(RuviError::Unsupported(
            "DASH MPD contains no playable representations".to_string(),
        ));
    }

    let title = mpd_url
        .path_segments()
        .and_then(|mut s| s.rfind(|seg| !seg.is_empty()))
        .and_then(|n| n.strip_suffix(".mpd").or(Some(n)))
        .filter(|s| !s.is_empty())
        .unwrap_or("dash_media")
        .to_string();

    let default_ext = streams
        .first()
        .map(|s| s.container.clone())
        .unwrap_or_else(|| "mp4".to_string());

    Ok(MediaSource {
        url: original_url.to_string(),
        resolved_url: mpd_url.to_string(),
        kind: SourceKind::DashVod,
        title,
        default_ext,
        streams,
        subtitles,
    })
}

fn determine_stream_kind(content_type: Option<&str>, mime_type: Option<&str>) -> StreamKind {
    if let Some(ct) = content_type {
        if ct.eq_ignore_ascii_case("audio") {
            return StreamKind::Audio;
        }
        if ct.eq_ignore_ascii_case("video") {
            return StreamKind::Video;
        }
    }
    if let Some(mime) = mime_type
        && mime.starts_with("audio/")
    {
        return StreamKind::Audio;
    }
    StreamKind::VideoAudio
}

fn join_base_url(current: &Url, rel: Option<&str>) -> RuviResult<Url> {
    match rel {
        Some(r) if !r.trim().is_empty() => current
            .join(r.trim())
            .map_err(|err| RuviError::Unsupported(format!("invalid DASH BaseURL '{r}': {err}"))),
        _ => Ok(current.clone()),
    }
}

fn merge_templates(
    parent: Option<&SegmentTemplateSpec>,
    child: Option<&SegmentTemplateSpec>,
) -> Option<SegmentTemplateSpec> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => Some(SegmentTemplateSpec {
            initialization: c
                .initialization
                .clone()
                .or_else(|| p.initialization.clone()),
            media: c.media.clone().or_else(|| p.media.clone()),
            start_number: c.start_number.or(p.start_number),
            timescale: c.timescale.or(p.timescale),
            duration: c.duration.or(p.duration),
            timeline: if c.timeline.is_empty() {
                p.timeline.clone()
            } else {
                c.timeline.clone()
            },
        }),
    }
}
