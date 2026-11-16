use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{RuviError, RuviResult};
use crate::resolver::dash::mpd::{
    AdaptationSet, MpdDocument, Period, Representation, SegmentListSpec, SegmentTemplateSpec,
    SegmentTimelineEntry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Mpd,
    Period,
    AdaptationSet,
    Representation,
    SegmentTemplate,
    SegmentList,
    BaseUrl,
}

pub fn parse_mpd_xml(xml: &str) -> RuviResult<MpdDocument> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut doc = MpdDocument {
        mpd_type: "static".to_string(),
        ..Default::default()
    };
    let mut stack: Vec<Scope> = Vec::new();
    let mut cur_period: Option<Period> = None;
    let mut cur_aset: Option<AdaptationSet> = None;
    let mut cur_rep: Option<Representation> = None;
    let mut cur_tpl: Option<SegmentTemplateSpec> = None;
    let mut cur_list: Option<SegmentListSpec> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let local = local_name(&name);
                match local {
                    "ContentProtection" => {
                        return Err(RuviError::Unsupported(
                            "DASH ContentProtection (DRM) detected; DRM circumvention is out of scope"
                                .to_string(),
                        ));
                    }
                    "MPD" => {
                        if let Some(t) = attr(&e, "type") {
                            doc.mpd_type = t;
                        }
                        doc.media_duration_seconds = attr(&e, "mediaPresentationDuration")
                            .and_then(|d| parse_iso_duration(&d));
                        stack.push(Scope::Mpd);
                    }
                    "Period" => {
                        let p = Period {
                            duration_seconds: attr(&e, "duration")
                                .and_then(|d| parse_iso_duration(&d)),
                            ..Default::default()
                        };
                        cur_period = Some(p);
                        stack.push(Scope::Period);
                    }
                    "AdaptationSet" => {
                        let a = AdaptationSet {
                            content_type: attr(&e, "contentType"),
                            mime_type: attr(&e, "mimeType"),
                            lang: attr(&e, "lang"),
                            ..Default::default()
                        };
                        cur_aset = Some(a);
                        stack.push(Scope::AdaptationSet);
                    }
                    "Representation" => {
                        let r = Representation {
                            id: attr(&e, "id").unwrap_or_else(|| "rep-0".to_string()),
                            bandwidth: attr(&e, "bandwidth").and_then(|v| v.parse().ok()),
                            width: attr(&e, "width").and_then(|v| v.parse().ok()),
                            height: attr(&e, "height").and_then(|v| v.parse().ok()),
                            codecs: attr(&e, "codecs"),
                            frame_rate: attr(&e, "frameRate"),
                            mime_type: attr(&e, "mimeType"),
                            ..Default::default()
                        };
                        cur_rep = Some(r);
                        stack.push(Scope::Representation);
                    }
                    "SegmentTemplate" => {
                        cur_tpl = Some(parse_segment_template_attrs(&e));
                        stack.push(Scope::SegmentTemplate);
                    }
                    "SegmentList" => {
                        cur_list = Some(SegmentListSpec::default());
                        stack.push(Scope::SegmentList);
                    }
                    "S" => {
                        if let Some(ref mut tpl) = cur_tpl
                            && let Some(d) = attr(&e, "d").and_then(|v| v.parse::<u64>().ok())
                        {
                            tpl.timeline.push(SegmentTimelineEntry {
                                start_time: attr(&e, "t").and_then(|v| v.parse().ok()),
                                duration: d,
                                repeat: attr(&e, "r").and_then(|v| v.parse().ok()).unwrap_or(0),
                            });
                        }
                        stack.push(Scope::SegmentTemplate);
                    }
                    "BaseURL" => {
                        stack.push(Scope::BaseUrl);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match local_name(&name) {
                    "ContentProtection" => {
                        return Err(RuviError::Unsupported(
                            "DASH ContentProtection (DRM) detected; DRM circumvention is out of scope"
                                .to_string(),
                        ));
                    }
                    "SegmentTemplate" => {
                        let tpl = parse_segment_template_attrs(&e);
                        assign_segment_template(tpl, &mut cur_rep, &mut cur_aset);
                    }
                    "S" => {
                        if let Some(ref mut tpl) = cur_tpl
                            && let Some(d) = attr(&e, "d").and_then(|v| v.parse::<u64>().ok())
                        {
                            tpl.timeline.push(SegmentTimelineEntry {
                                start_time: attr(&e, "t").and_then(|v| v.parse().ok()),
                                duration: d,
                                repeat: attr(&e, "r").and_then(|v| v.parse().ok()).unwrap_or(0),
                            });
                        }
                    }
                    "Initialization" => {
                        if let Some(ref mut list) = cur_list
                            && let Some(src) = attr(&e, "sourceURL")
                        {
                            list.initialization = Some(src);
                        }
                    }
                    "SegmentURL" => {
                        if let Some(ref mut list) = cur_list
                            && let Some(media) = attr(&e, "media")
                        {
                            list.segments.push(media);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if stack.last() == Some(&Scope::BaseUrl) {
                    let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                    if !text.is_empty() {
                        // Assign to innermost open container
                        let parent_scope = stack.iter().rev().nth(1).copied();
                        match parent_scope {
                            Some(Scope::Representation) => {
                                if let Some(ref mut rep) = cur_rep {
                                    rep.base_url = Some(text);
                                }
                            }
                            Some(Scope::AdaptationSet) => {
                                if let Some(ref mut aset) = cur_aset {
                                    aset.base_url = Some(text);
                                }
                            }
                            Some(Scope::Period) => {
                                if let Some(ref mut p) = cur_period {
                                    p.base_url = Some(text);
                                }
                            }
                            Some(Scope::Mpd) => {
                                doc.base_url = Some(text);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match local_name(&name) {
                    "BaseURL" => {
                        if stack.last() == Some(&Scope::BaseUrl) {
                            stack.pop();
                        }
                    }
                    "S" => {
                        if stack.last() == Some(&Scope::SegmentTemplate) {
                            stack.pop();
                        }
                    }
                    "SegmentTemplate" => {
                        if let Some(tpl) = cur_tpl.take() {
                            assign_segment_template(tpl, &mut cur_rep, &mut cur_aset);
                        }
                        if stack.last() == Some(&Scope::SegmentTemplate) {
                            stack.pop();
                        }
                    }
                    "SegmentList" => {
                        if let (Some(list), Some(rep)) = (cur_list.take(), cur_rep.as_mut()) {
                            rep.segment_list = Some(list);
                        }
                        if stack.last() == Some(&Scope::SegmentList) {
                            stack.pop();
                        }
                    }
                    "Representation" => {
                        if let (Some(rep), Some(aset)) = (cur_rep.take(), cur_aset.as_mut()) {
                            aset.representations.push(rep);
                        }
                        if stack.last() == Some(&Scope::Representation) {
                            stack.pop();
                        }
                    }
                    "AdaptationSet" => {
                        if let (Some(aset), Some(p)) = (cur_aset.take(), cur_period.as_mut()) {
                            p.adaptation_sets.push(aset);
                        }
                        if stack.last() == Some(&Scope::AdaptationSet) {
                            stack.pop();
                        }
                    }
                    "Period" => {
                        if let Some(p) = cur_period.take() {
                            doc.periods.push(p);
                        }
                        if stack.last() == Some(&Scope::Period) {
                            stack.pop();
                        }
                    }
                    "MPD" if stack.last() == Some(&Scope::Mpd) => {
                        stack.pop();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(RuviError::Unsupported(format!(
                    "failed to parse DASH MPD XML: {err}"
                )));
            }
            _ => {}
        }
    }

    if doc.periods.is_empty() {
        return Err(RuviError::Unsupported(
            "invalid DASH MPD: no <Period> element found".to_string(),
        ));
    }

    Ok(doc)
}

fn assign_segment_template(
    tpl: SegmentTemplateSpec,
    rep: &mut Option<Representation>,
    aset: &mut Option<AdaptationSet>,
) {
    if let Some(r) = rep.as_mut() {
        r.segment_template = Some(tpl);
    } else if let Some(a) = aset.as_mut() {
        a.segment_template = Some(tpl);
    }
}

fn parse_segment_template_attrs(e: &BytesStart<'_>) -> SegmentTemplateSpec {
    SegmentTemplateSpec {
        initialization: attr(e, "initialization"),
        media: attr(e, "media"),
        start_number: attr(e, "startNumber").and_then(|v| v.parse().ok()),
        timescale: attr(e, "timescale").and_then(|v| v.parse().ok()),
        duration: attr(e, "duration").and_then(|v| v.parse().ok()),
        timeline: Vec::new(),
    }
}

fn attr(e: &BytesStart<'_>, target: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref());
        if local_name(&key) == target {
            return String::from_utf8(a.value.into_owned()).ok();
        }
    }
    None
}

fn local_name(full: &str) -> &str {
    full.rsplit_once(':').map_or(full, |(_, local)| local)
}

/// Parses ISO 8601 durations like `PT12.5S`, `PT1M30S`, `PT1H2M3.5S`.
pub fn parse_iso_duration(raw: &str) -> Option<f64> {
    let s = raw.trim().strip_prefix("PT")?;
    let mut total = 0.0f64;
    let mut num_buf = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num_buf.push(ch);
        } else {
            let val = num_buf.parse::<f64>().ok()?;
            num_buf.clear();
            match ch {
                'H' => total += val * 3600.0,
                'M' => total += val * 60.0,
                'S' => total += val,
                _ => return None,
            }
        }
    }
    Some(total)
}
