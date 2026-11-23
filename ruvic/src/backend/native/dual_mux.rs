//! Lossless, streaming dual-input (video + audio) muxing for real-world adaptive
//! sources (fragmented MP4 / DASH and WebM / Matroska), written from scratch in
//! pure Rust. Every function validates its inputs completely before writing a
//! single byte, returning `Ok(false)` so callers can fall back to other muxers.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::backend::native::mp4_atoms::{build_mvhd, copy_chunked, wrap_box};
use crate::error::RuviResult;

// ----------------------------------------------------------------------------
// Fragmented MP4
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct TopBox {
    kind: [u8; 4],
    offset: u64,
    size: u64,
    header: u64,
}

fn scan_top_level_boxes<R: Read + Seek>(r: &mut R) -> RuviResult<Option<Vec<TopBox>>> {
    let len = r.seek(SeekFrom::End(0))?;
    let mut out = Vec::new();
    let mut pos = 0u64;
    while pos + 8 <= len {
        r.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 16];
        let avail = (len - pos).min(16) as usize;
        r.read_exact(&mut hdr[..avail])?;
        let size32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let kind = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let (size, header) = match size32 {
            0 => (len - pos, 8),
            1 => {
                if avail < 16 {
                    return Ok(None);
                }
                (
                    u64::from_be_bytes(hdr[8..16].try_into().unwrap_or([0; 8])),
                    16,
                )
            }
            s => (s, 8),
        };
        if size < header || size > len - pos {
            return Ok(None);
        }
        out.push(TopBox {
            kind,
            offset: pos,
            size,
            header,
        });
        pos += size;
    }
    Ok(Some(out))
}

/// Returns `(kind, start, end)` of child boxes inside `body`.
fn child_boxes(body: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= body.len() {
        let size =
            u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]) as usize;
        let kind = [body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]];
        let size = if size == 0 { body.len() - pos } else { size };
        if size < 8 || pos + size > body.len() {
            break;
        }
        out.push((kind, pos, pos + size));
        pos += size;
    }
    out
}

fn find_child(body: &[u8], kind: &[u8; 4]) -> Option<(usize, usize)> {
    child_boxes(body)
        .into_iter()
        .find(|(k, _, _)| k == kind)
        .map(|(_, s, e)| (s, e))
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_u64(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8)
        .map(|s| u64::from_be_bytes(s.try_into().unwrap_or([0; 8])))
}

struct Fmp4Track {
    trak: Vec<u8>,
    trex: Option<Vec<u8>>,
    timescale: u32,
    duration_secs: f64,
    fragments: Vec<(u64, u64, u64, u64, u64)>, // decode_time, moof_off, moof_len, mdat_off, mdat_len
}

#[derive(Debug, Clone, Copy)]
struct DefragSample {
    file_offset: u64,
    size: u32,
    duration: u32,
    cts_offset: i32,
    is_sync: bool,
    dts: u64,
}

#[derive(Debug, Clone)]
struct DefragTrack {
    orig_trak: Vec<u8>,
    track_id: u32,
    timescale: u32,
    is_audio: bool,
    has_cts: bool,
    samples: Vec<DefragSample>,
}

fn trak_handler_type(trak_body: &[u8]) -> Option<[u8; 4]> {
    let (ms, me) = find_child(trak_body, b"mdia")?;
    let mdia = &trak_body[ms + 8..me];
    let (hs, he) = find_child(mdia, b"hdlr")?;
    let hdlr = &mdia[hs + 8..he];
    if hdlr.len() < 12 {
        return None;
    }
    Some([hdlr[8], hdlr[9], hdlr[10], hdlr[11]])
}

fn parse_trex_defaults(moov_body: &[u8], target_tid: u32) -> (u32, u32, u32) {
    let Some((ms, me)) = find_child(moov_body, b"mvex") else {
        return (0, 0, 0);
    };
    let mvex = &moov_body[ms + 8..me];
    for (k, s, e) in child_boxes(mvex) {
        if &k == b"trex" {
            let p = &mvex[s + 8..e];
            if p.len() >= 24 {
                let tid = read_u32(p, 4).unwrap_or(0);
                if tid == target_tid || target_tid == 0 {
                    let def_dur = read_u32(p, 12).unwrap_or(0);
                    let def_size = read_u32(p, 16).unwrap_or(0);
                    let def_flags = read_u32(p, 20).unwrap_or(0);
                    return (def_dur, def_size, def_flags);
                }
            }
        }
    }
    (0, 0, 0)
}

#[allow(clippy::too_many_arguments)]
fn parse_moof_samples_for_track(
    moof: &[u8],
    moof_offset: u64,
    mdat_box: TopBox,
    expected_tid: Option<u32>,
    trex_defaults: (u32, u32, u32),
    is_audio: bool,
    fallback_dts: u64,
    out_samples: &mut Vec<DefragSample>,
    out_has_cts: &mut bool,
) -> Option<u32> {
    if moof.len() < 8 {
        return None;
    }
    let body = &moof[8..];
    let mut matched_tid = None;
    for (kind, ts, te) in child_boxes(body) {
        if &kind != b"traf" {
            continue;
        }
        let traf = &body[ts + 8..te];
        let (hs, he) = find_child(traf, b"tfhd")?;
        let tfhd = &traf[hs + 8..he];
        if tfhd.len() < 8 {
            continue;
        }
        let tf_flags = u32::from_be_bytes([0, tfhd[1], tfhd[2], tfhd[3]]);
        let tid = read_u32(tfhd, 4)?;
        if let Some(exp) = expected_tid
            && tid != exp
        {
            continue;
        }
        matched_tid = Some(tid);
        let mut pos = 8usize;
        let base_data_offset = if (tf_flags & 0x000001) != 0 {
            let v = read_u64(tfhd, pos)?;
            pos += 8;
            Some(v)
        } else {
            None
        };
        if (tf_flags & 0x000002) != 0 {
            pos += 4;
        }
        let def_dur = if (tf_flags & 0x000008) != 0 {
            let v = read_u32(tfhd, pos)?;
            pos += 4;
            v
        } else {
            trex_defaults.0
        };
        let def_size = if (tf_flags & 0x000010) != 0 {
            let v = read_u32(tfhd, pos)?;
            pos += 4;
            v
        } else {
            trex_defaults.1
        };
        let def_flags = if (tf_flags & 0x000020) != 0 {
            read_u32(tfhd, pos)?
        } else {
            trex_defaults.2
        };

        let mut dts = find_child(traf, b"tfdt")
            .and_then(|(ds, de)| {
                let p = &traf[ds + 8..de];
                match p.first() {
                    Some(1) => read_u64(p, 4),
                    _ => read_u32(p, 4).map(u64::from),
                }
            })
            .unwrap_or(fallback_dts);

        let mdat_payload_start = mdat_box.offset + mdat_box.header;
        let mdat_end = mdat_box.offset + mdat_box.size;
        let mut running_offset = base_data_offset.unwrap_or(mdat_payload_start);

        for (tk, trs, tre) in child_boxes(traf) {
            if &tk != b"trun" {
                continue;
            }
            let trun = &traf[trs + 8..tre];
            if trun.len() < 8 {
                return None;
            }
            let tr_ver = trun[0];
            let tr_flags = u32::from_be_bytes([0, trun[1], trun[2], trun[3]]);
            let sample_count = read_u32(trun, 4)? as usize;
            let mut tpos = 8usize;
            if (tr_flags & 0x000001) != 0 {
                let rel = read_u32(trun, tpos)? as i32;
                tpos += 4;
                let base = base_data_offset.unwrap_or(moof_offset) as i64;
                let computed = (base + i64::from(rel)) as u64;
                running_offset = if computed >= mdat_payload_start && computed < mdat_end {
                    computed
                } else {
                    mdat_payload_start
                };
            }
            let first_sample_flags = if (tr_flags & 0x000004) != 0 {
                let v = read_u32(trun, tpos)?;
                tpos += 4;
                Some(v)
            } else {
                None
            };

            let has_dur = (tr_flags & 0x000100) != 0;
            let has_size = (tr_flags & 0x000200) != 0;
            let has_flags = (tr_flags & 0x000400) != 0;
            let has_cts = (tr_flags & 0x000800) != 0;
            if has_cts {
                *out_has_cts = true;
            }

            let fallback_dur = if def_dur > 0 {
                def_dur
            } else if is_audio {
                1024
            } else {
                512
            };

            for s_idx in 0..sample_count {
                let dur = if has_dur {
                    let v = read_u32(trun, tpos)?;
                    tpos += 4;
                    if v > 0 { v } else { fallback_dur }
                } else {
                    fallback_dur
                };
                let size = if has_size {
                    let v = read_u32(trun, tpos)?;
                    tpos += 4;
                    v
                } else if def_size > 0 {
                    def_size
                } else if sample_count == 1 {
                    mdat_box.size.saturating_sub(mdat_box.header) as u32
                } else {
                    return None;
                };
                let s_flags = if has_flags {
                    let v = read_u32(trun, tpos)?;
                    tpos += 4;
                    v
                } else if s_idx == 0 {
                    first_sample_flags.unwrap_or(def_flags)
                } else {
                    def_flags
                };
                let cts_offset = if has_cts {
                    let raw = read_u32(trun, tpos)?;
                    tpos += 4;
                    if tr_ver == 0 {
                        (raw as i32).max(0)
                    } else {
                        raw as i32
                    }
                } else {
                    0
                };

                if size == 0 || running_offset.saturating_add(u64::from(size)) > mdat_end {
                    return None;
                }
                let is_non_sync = (s_flags & 0x0001_0000) != 0 || ((s_flags >> 24) & 0x03) == 1;
                let is_sync = if is_audio {
                    true
                } else if has_flags || first_sample_flags.is_some() || def_flags != 0 {
                    !is_non_sync
                } else {
                    s_idx == 0
                };

                out_samples.push(DefragSample {
                    file_offset: running_offset,
                    size,
                    duration: dur,
                    cts_offset,
                    is_sync,
                    dts,
                });
                running_offset += u64::from(size);
                dts = dts.saturating_add(u64::from(dur));
            }
        }
    }
    matched_tid
}

fn collect_defrag_tracks<R: Read + Seek>(r: &mut R) -> RuviResult<Option<Vec<DefragTrack>>> {
    let Some(boxes) = scan_top_level_boxes(r)? else {
        return Ok(None);
    };
    let Some(moov_box) = boxes.iter().find(|b| &b.kind == b"moov").copied() else {
        return Ok(None);
    };
    if !boxes.iter().any(|b| &b.kind == b"moof") || moov_box.size > 64 * 1024 * 1024 {
        return Ok(None);
    }
    let mut moov = vec![0u8; moov_box.size as usize];
    r.seek(SeekFrom::Start(moov_box.offset))?;
    r.read_exact(&mut moov)?;
    let moov_body = &moov[moov_box.header as usize..];

    let mut tracks: Vec<(DefragTrack, (u32, u32, u32))> = Vec::new();
    for (k, s, e) in child_boxes(moov_body) {
        if &k == b"trak" {
            let orig_trak = moov_body[s..e].to_vec();
            let tbody = &orig_trak[8..];
            let tid = tkhd_track_id(tbody).unwrap_or((tracks.len() + 1) as u32);
            let ts = mdhd_timescale(tbody).unwrap_or(1000).max(1);
            let htype = trak_handler_type(tbody).unwrap_or(*b"vide");
            let is_audio = &htype == b"soun";
            let defaults = parse_trex_defaults(moov_body, tid);
            tracks.push((
                DefragTrack {
                    orig_trak,
                    track_id: tid,
                    timescale: ts,
                    is_audio,
                    has_cts: false,
                    samples: Vec::new(),
                },
                defaults,
            ));
        }
    }
    if tracks.is_empty() {
        return Ok(None);
    }
    let single_track = tracks.len() == 1;

    let mut i = 0usize;
    while i < boxes.len() {
        if &boxes[i].kind == b"moof" {
            let Some(mdat) = boxes.get(i + 1).copied().filter(|b| &b.kind == b"mdat") else {
                return Ok(None);
            };
            if boxes[i].size > 16 * 1024 * 1024 {
                return Ok(None);
            }
            let mut moof_bytes = vec![0u8; boxes[i].size as usize];
            r.seek(SeekFrom::Start(boxes[i].offset))?;
            r.read_exact(&mut moof_bytes)?;

            let moof_tid = tfhd_track_id(&moof_bytes);
            let target_idx = if single_track {
                Some(0usize)
            } else {
                moof_tid.and_then(|id| tracks.iter().position(|(t, _)| t.track_id == id))
            };
            let Some(t_idx) = target_idx else {
                i += 2;
                continue;
            };
            let (trk, defaults) = &mut tracks[t_idx];
            let fallback_dts = trk
                .samples
                .last()
                .map(|s| s.dts.saturating_add(u64::from(s.duration)))
                .unwrap_or(0);
            let expected = if single_track {
                None
            } else {
                Some(trk.track_id)
            };
            if parse_moof_samples_for_track(
                &moof_bytes,
                boxes[i].offset,
                mdat,
                expected,
                *defaults,
                trk.is_audio,
                fallback_dts,
                &mut trk.samples,
                &mut trk.has_cts,
            )
            .is_none()
            {
                return Ok(None);
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    let result: Vec<DefragTrack> = tracks
        .into_iter()
        .map(|(t, _)| t)
        .filter(|t| !t.samples.is_empty())
        .collect();
    if result.is_empty() {
        return Ok(None);
    }
    Ok(Some(result))
}

fn build_populated_stbl(
    orig_stbl_body: &[u8],
    samples: &[DefragSample],
    chunk_offset: u64,
    is_audio: bool,
    has_cts: bool,
) -> Option<Vec<u8>> {
    let (stsd_s, stsd_e) = find_child(orig_stbl_body, b"stsd")?;
    let stsd_box = &orig_stbl_body[stsd_s..stsd_e];

    // 1. Run-length encode stts
    let mut stts_runs: Vec<(u32, u32)> = Vec::new();
    for s in samples {
        let d = s.duration.max(1);
        if let Some(last) = stts_runs.last_mut()
            && last.1 == d
        {
            last.0 += 1;
        } else {
            stts_runs.push((1, d));
        }
    }
    let mut stts_body = Vec::with_capacity(8 + stts_runs.len() * 8);
    stts_body.extend_from_slice(&0u32.to_be_bytes());
    stts_body.extend_from_slice(&(stts_runs.len() as u32).to_be_bytes());
    for (cnt, delta) in stts_runs {
        stts_body.extend_from_slice(&cnt.to_be_bytes());
        stts_body.extend_from_slice(&delta.to_be_bytes());
    }

    // 2. Optional ctts
    let ctts_box = if has_cts && samples.iter().any(|s| s.cts_offset != 0) {
        let use_v1 = samples.iter().any(|s| s.cts_offset < 0);
        let mut ctts_runs: Vec<(u32, i32)> = Vec::new();
        for s in samples {
            if let Some(last) = ctts_runs.last_mut()
                && last.1 == s.cts_offset
            {
                last.0 += 1;
            } else {
                ctts_runs.push((1, s.cts_offset));
            }
        }
        let mut ctts_body = Vec::with_capacity(8 + ctts_runs.len() * 8);
        let ver_flags: u32 = if use_v1 { 0x0100_0000 } else { 0 };
        ctts_body.extend_from_slice(&ver_flags.to_be_bytes());
        ctts_body.extend_from_slice(&(ctts_runs.len() as u32).to_be_bytes());
        for (cnt, off) in ctts_runs {
            ctts_body.extend_from_slice(&cnt.to_be_bytes());
            ctts_body.extend_from_slice(&(off as u32).to_be_bytes());
        }
        Some(wrap_box(b"ctts", &ctts_body))
    } else {
        None
    };

    // 3. Optional stss for video keyframes
    let stss_box = if !is_audio {
        let mut syncs: Vec<u32> = samples
            .iter()
            .enumerate()
            .filter_map(|(idx, s)| s.is_sync.then_some((idx + 1) as u32))
            .collect();
        if syncs.is_empty() {
            syncs.push(1);
        }
        let mut stss_body = Vec::with_capacity(8 + syncs.len() * 4);
        stss_body.extend_from_slice(&0u32.to_be_bytes());
        stss_body.extend_from_slice(&(syncs.len() as u32).to_be_bytes());
        for idx in syncs {
            stss_body.extend_from_slice(&idx.to_be_bytes());
        }
        Some(wrap_box(b"stss", &stss_body))
    } else {
        None
    };

    // 4. stsc: 1 contiguous chunk holding all samples.len() samples
    let mut stsc_body = Vec::with_capacity(20);
    stsc_body.extend_from_slice(&0u32.to_be_bytes());
    stsc_body.extend_from_slice(&1u32.to_be_bytes());
    stsc_body.extend_from_slice(&1u32.to_be_bytes());
    stsc_body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    stsc_body.extend_from_slice(&1u32.to_be_bytes());

    // 5. stsz: individual sample sizes
    let mut stsz_body = Vec::with_capacity(12 + samples.len() * 4);
    stsz_body.extend_from_slice(&0u32.to_be_bytes());
    stsz_body.extend_from_slice(&0u32.to_be_bytes());
    stsz_body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        stsz_body.extend_from_slice(&s.size.to_be_bytes());
    }

    // 6. stco (always 12 bytes for < 4 GB offsets)
    let co_box = if chunk_offset > u64::from(u32::MAX) {
        let mut co64 = Vec::with_capacity(16);
        co64.extend_from_slice(&0u32.to_be_bytes());
        co64.extend_from_slice(&1u32.to_be_bytes());
        co64.extend_from_slice(&chunk_offset.to_be_bytes());
        wrap_box(b"co64", &co64)
    } else {
        let mut stco = Vec::with_capacity(12);
        stco.extend_from_slice(&0u32.to_be_bytes());
        stco.extend_from_slice(&1u32.to_be_bytes());
        stco.extend_from_slice(&(chunk_offset as u32).to_be_bytes());
        wrap_box(b"stco", &stco)
    };

    let mut out = Vec::with_capacity(stsd_box.len() + stts_body.len() + stsz_body.len() + 128);
    out.extend_from_slice(stsd_box);
    out.extend_from_slice(&wrap_box(b"stts", &stts_body));
    if let Some(ctts) = ctts_box {
        out.extend_from_slice(&ctts);
    }
    if let Some(stss) = stss_box {
        out.extend_from_slice(&stss);
    }
    out.extend_from_slice(&wrap_box(b"stsc", &stsc_body));
    out.extend_from_slice(&wrap_box(b"stsz", &stsz_body));
    out.extend_from_slice(&co_box);
    Some(wrap_box(b"stbl", &out))
}

fn rebuild_defrag_trak(
    trk: &DefragTrack,
    new_track_id: u32,
    movie_timescale: u32,
    chunk_offset: u64,
) -> Option<Vec<u8>> {
    if trk.orig_trak.len() < 8 || trk.samples.is_empty() {
        return None;
    }
    let media_dur_ticks: u64 = trk
        .samples
        .iter()
        .map(|s| u64::from(s.duration.max(1)))
        .sum();
    let movie_dur_units = ((media_dur_ticks as f64 / f64::from(trk.timescale.max(1)))
        * f64::from(movie_timescale))
    .round()
    .clamp(1.0, f64::from(u32::MAX)) as u32;

    let tbody = &trk.orig_trak[8..];
    let mut new_trak_body = Vec::with_capacity(tbody.len() + trk.samples.len() * 4 + 128);

    for (k, s, e) in child_boxes(tbody) {
        if &k == b"edts" {
            // Strip stale DASH single-track edit lists so A/V tracks start cleanly at t=0.
            continue;
        } else if &k == b"tkhd" {
            let mut tkhd = tbody[s..e].to_vec();
            let ver = tkhd.get(8).copied().unwrap_or(0);
            // Enable Track_enabled | Track_in_movie | Track_in_preview (0x07)
            if tkhd.len() >= 12 {
                tkhd[11] = 0x07;
            }
            if ver == 1 && tkhd.len() >= 44 {
                tkhd[28..32].copy_from_slice(&new_track_id.to_be_bytes());
                tkhd[36..44].copy_from_slice(&u64::from(movie_dur_units).to_be_bytes());
            } else if ver == 0 && tkhd.len() >= 32 {
                tkhd[20..24].copy_from_slice(&new_track_id.to_be_bytes());
                tkhd[28..32].copy_from_slice(&movie_dur_units.to_be_bytes());
            }
            new_trak_body.extend_from_slice(&tkhd);
        } else if &k == b"mdia" {
            let mdia_body = &tbody[s + 8..e];
            let mut new_mdia_body = Vec::with_capacity(mdia_body.len() + trk.samples.len() * 4);
            for (mk, ms, me) in child_boxes(mdia_body) {
                if &mk == b"mdhd" {
                    let mut mdhd = mdia_body[ms..me].to_vec();
                    let ver = mdhd.get(8).copied().unwrap_or(0);
                    if ver == 1 && mdhd.len() >= 36 {
                        mdhd[28..36].copy_from_slice(&media_dur_ticks.to_be_bytes());
                    } else if ver == 0 && mdhd.len() >= 28 {
                        let d32 = u32::try_from(media_dur_ticks).unwrap_or(u32::MAX);
                        mdhd[24..28].copy_from_slice(&d32.to_be_bytes());
                    }
                    new_mdia_body.extend_from_slice(&mdhd);
                } else if &mk == b"minf" {
                    let minf_body = &mdia_body[ms + 8..me];
                    let mut new_minf_body =
                        Vec::with_capacity(minf_body.len() + trk.samples.len() * 4);
                    for (ik, is, ie) in child_boxes(minf_body) {
                        if &ik == b"stbl" {
                            let stbl_body = &minf_body[is + 8..ie];
                            let new_stbl = build_populated_stbl(
                                stbl_body,
                                &trk.samples,
                                chunk_offset,
                                trk.is_audio,
                                trk.has_cts,
                            )?;
                            new_minf_body.extend_from_slice(&new_stbl);
                        } else {
                            new_minf_body.extend_from_slice(&minf_body[is..ie]);
                        }
                    }
                    new_mdia_body.extend_from_slice(&wrap_box(b"minf", &new_minf_body));
                } else {
                    new_mdia_body.extend_from_slice(&mdia_body[ms..me]);
                }
            }
            new_trak_body.extend_from_slice(&wrap_box(b"mdia", &new_mdia_body));
        } else {
            new_trak_body.extend_from_slice(&tbody[s..e]);
        }
    }
    Some(wrap_box(b"trak", &new_trak_body))
}

fn copy_exact_range<R: Read + Seek, W: Write>(
    r: &mut R,
    w: &mut W,
    start_offset: u64,
    byte_len: u64,
    buf: &mut [u8],
) -> RuviResult<()> {
    r.seek(SeekFrom::Start(start_offset))?;
    let mut rem = byte_len;
    while rem > 0 {
        let step = rem.min(buf.len() as u64) as usize;
        r.read_exact(&mut buf[..step])?;
        w.write_all(&buf[..step])?;
        rem -= step as u64;
    }
    Ok(())
}

fn stream_track_samples<R: Read + Seek, W: Write>(
    r: &mut R,
    w: &mut W,
    samples: &[DefragSample],
) -> RuviResult<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let mut buf = vec![0u8; 256 * 1024];
    let mut run_start = samples[0].file_offset;
    let mut run_len = u64::from(samples[0].size);
    for s in &samples[1..] {
        if s.file_offset == run_start + run_len {
            run_len += u64::from(s.size);
        } else {
            copy_exact_range(r, w, run_start, run_len, &mut buf)?;
            run_start = s.file_offset;
            run_len = u64::from(s.size);
        }
    }
    copy_exact_range(r, w, run_start, run_len, &mut buf)?;
    Ok(())
}

fn mdhd_timescale(trak_body: &[u8]) -> Option<u32> {
    let (ms, me) = find_child(trak_body, b"mdia")?;
    let mdia = &trak_body[ms + 8..me];
    let (hs, he) = find_child(mdia, b"mdhd")?;
    let mdhd = &mdia[hs + 8..he];
    match mdhd.first()? {
        1 => read_u32(mdhd, 20),
        _ => read_u32(mdhd, 12),
    }
}

fn mvhd_seconds(moov_body: &[u8]) -> f64 {
    let Some((s, e)) = find_child(moov_body, b"mvhd") else {
        return 0.0;
    };
    let p = &moov_body[s + 8..e];
    let (ts, dur) = match p.first() {
        Some(1) => (read_u32(p, 20), read_u64(p, 24)),
        _ => (read_u32(p, 12), read_u32(p, 16).map(u64::from)),
    };
    match (ts, dur) {
        (Some(ts), Some(d)) if ts > 0 && d != u64::MAX && d != u64::from(u32::MAX) => {
            d as f64 / f64::from(ts)
        }
        _ => 0.0,
    }
}

fn mehd_seconds(moov_body: &[u8], movie_timescale: u32) -> f64 {
    let Some((s, e)) = find_child(moov_body, b"mvex") else {
        return 0.0;
    };
    let mvex = &moov_body[s + 8..e];
    let Some((hs, he)) = find_child(mvex, b"mehd") else {
        return 0.0;
    };
    let p = &mvex[hs + 8..he];
    let d = match p.first() {
        Some(1) => read_u64(p, 4),
        _ => read_u32(p, 4).map(u64::from),
    };
    match d {
        Some(d) if movie_timescale > 0 => d as f64 / f64::from(movie_timescale),
        _ => 0.0,
    }
}

fn movie_timescale(moov_body: &[u8]) -> u32 {
    find_child(moov_body, b"mvhd")
        .and_then(|(s, e)| {
            let p = &moov_body[s + 8..e];
            match p.first() {
                Some(1) => read_u32(p, 20),
                _ => read_u32(p, 12),
            }
        })
        .unwrap_or(1000)
}

fn tfdt_decode_time(moof: &[u8]) -> u64 {
    let body = &moof[8.min(moof.len())..];
    let Some((ts, te)) = find_child(body, b"traf") else {
        return 0;
    };
    let traf = &body[ts + 8..te];
    let Some((ds, de)) = find_child(traf, b"tfdt") else {
        return 0;
    };
    let p = &traf[ds + 8..de];
    match p.first() {
        Some(1) => read_u64(p, 4).unwrap_or(0),
        _ => read_u32(p, 4).map(u64::from).unwrap_or(0),
    }
}

/// Patches `mfhd.sequence_number` and every `traf/tfhd.track_ID` in place.
fn patch_moof(moof: &mut [u8], seq: u32, track_id: u32) {
    if moof.len() < 8 {
        return;
    }
    let children = child_boxes(&moof[8..]);
    for (kind, s, e) in children {
        let abs = 8 + s;
        if &kind == b"mfhd" && abs + 16 <= 8 + e {
            moof[abs + 12..abs + 16].copy_from_slice(&seq.to_be_bytes());
        } else if &kind == b"traf" {
            let traf_body_start = abs + 8;
            let inner = child_boxes(&moof[traf_body_start..8 + e]);
            for (ik, is, ie) in inner {
                let iabs = traf_body_start + is;
                if &ik == b"tfhd" && iabs + 16 <= traf_body_start + ie {
                    moof[iabs + 12..iabs + 16].copy_from_slice(&track_id.to_be_bytes());
                }
            }
        }
    }
}

fn build_trex(track_id: u32) -> Vec<u8> {
    let mut b = vec![0u8; 24];
    b[4..8].copy_from_slice(&track_id.to_be_bytes());
    b[8..12].copy_from_slice(&1u32.to_be_bytes());
    wrap_box(b"trex", &b)
}

fn load_fmp4_track<R: Read + Seek>(r: &mut R) -> RuviResult<Option<Fmp4Track>> {
    let Some(boxes) = scan_top_level_boxes(r)? else {
        return Ok(None);
    };
    let Some(moov_box) = boxes.iter().find(|b| &b.kind == b"moov").copied() else {
        return Ok(None);
    };
    if moov_box.size > 64 * 1024 * 1024 {
        return Ok(None);
    }
    let mut moov = vec![0u8; moov_box.size as usize];
    r.seek(SeekFrom::Start(moov_box.offset))?;
    r.read_exact(&mut moov)?;
    let moov_body = &moov[moov_box.header as usize..];

    let traks: Vec<_> = child_boxes(moov_body)
        .into_iter()
        .filter(|(k, _, _)| k == b"trak")
        .collect();
    if traks.len() != 1 {
        return Ok(None);
    }
    let (_, ts, te) = traks[0];
    let trak = moov_body[ts..te].to_vec();
    let timescale = mdhd_timescale(&trak[8..]).unwrap_or(1000).max(1);

    let trex = find_child(moov_body, b"mvex").and_then(|(s, e)| {
        let mvex = &moov_body[s + 8..e];
        find_child(mvex, b"trex").map(|(ts, te)| mvex[ts..te].to_vec())
    });

    let mts = movie_timescale(moov_body);
    let duration_secs = mvhd_seconds(moov_body).max(mehd_seconds(moov_body, mts));

    let mut fragments = Vec::new();
    let mut i = 0;
    while i < boxes.len() {
        let b = boxes[i];
        if &b.kind == b"moof" {
            let Some(next) = boxes.get(i + 1).copied() else {
                return Ok(None);
            };
            if &next.kind != b"mdat" || b.size > 16 * 1024 * 1024 {
                return Ok(None);
            }
            let mut moof = vec![0u8; b.size as usize];
            r.seek(SeekFrom::Start(b.offset))?;
            r.read_exact(&mut moof)?;
            fragments.push((
                tfdt_decode_time(&moof),
                b.offset,
                b.size,
                next.offset,
                next.size,
            ));
            i += 2;
            continue;
        }
        i += 1;
    }
    if fragments.is_empty() {
        return Ok(None);
    }

    Ok(Some(Fmp4Track {
        trak,
        trex,
        timescale,
        duration_secs,
        fragments,
    }))
}

fn clean_fmp4_trak_for_mux(
    trak: &[u8],
    new_track_id: u32,
    movie_dur_ms: u32,
    timescale: u32,
) -> Vec<u8> {
    if trak.len() < 8 {
        return trak.to_vec();
    }
    let media_dur_ticks = ((f64::from(movie_dur_ms) / 1000.0) * f64::from(timescale.max(1)))
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u64;
    let tbody = &trak[8..];
    let mut new_tbody = Vec::with_capacity(tbody.len());
    for (k, s, e) in child_boxes(tbody) {
        if &k == b"edts" {
            // Strip stale single-track DASH edit lists so A/V tracks align at t=0.
            continue;
        } else if &k == b"tkhd" {
            let mut tkhd = tbody[s..e].to_vec();
            let ver = tkhd.get(8).copied().unwrap_or(0);
            if tkhd.len() >= 12 {
                tkhd[11] = 0x07;
            }
            if ver == 1 && tkhd.len() >= 44 {
                tkhd[28..32].copy_from_slice(&new_track_id.to_be_bytes());
                tkhd[36..44].copy_from_slice(&u64::from(movie_dur_ms).to_be_bytes());
            } else if ver == 0 && tkhd.len() >= 32 {
                tkhd[20..24].copy_from_slice(&new_track_id.to_be_bytes());
                tkhd[28..32].copy_from_slice(&movie_dur_ms.to_be_bytes());
            }
            new_tbody.extend_from_slice(&tkhd);
        } else if &k == b"mdia" {
            let mdia_body = &tbody[s + 8..e];
            let mut new_mdia = Vec::with_capacity(mdia_body.len());
            for (mk, ms, me) in child_boxes(mdia_body) {
                if &mk == b"mdhd" {
                    let mut mdhd = mdia_body[ms..me].to_vec();
                    let ver = mdhd.get(8).copied().unwrap_or(0);
                    if ver == 1 && mdhd.len() >= 36 {
                        mdhd[28..36].copy_from_slice(&media_dur_ticks.to_be_bytes());
                    } else if ver == 0 && mdhd.len() >= 28 {
                        let d32 = u32::try_from(media_dur_ticks).unwrap_or(u32::MAX);
                        mdhd[24..28].copy_from_slice(&d32.to_be_bytes());
                    }
                    new_mdia.extend_from_slice(&mdhd);
                } else {
                    new_mdia.extend_from_slice(&mdia_body[ms..me]);
                }
            }
            new_tbody.extend_from_slice(&wrap_box(b"mdia", &new_mdia));
        } else {
            new_tbody.extend_from_slice(&tbody[s..e]);
        }
    }
    wrap_box(b"trak", &new_tbody)
}

/// Defragments two single-track DASH fMP4 streams (video + audio) directly into a
/// standard non-fragmented Faststart MP4 (`ftyp` + `moov` with real sample tables + `mdat`),
/// falling back to clean interleaved fMP4 if any fragment omits `trun` sample tables.
pub fn try_mux_fmp4<R1: Read + Seek, R2: Read + Seek, W: Write>(
    video: &mut R1,
    audio: &mut R2,
    out: &mut W,
) -> RuviResult<bool> {
    if let (Ok(Some(v_tracks)), Ok(Some(a_tracks))) =
        (collect_defrag_tracks(video), collect_defrag_tracks(audio))
        && let (Some(v_trk), Some(a_trk)) = (v_tracks.first(), a_tracks.first())
        && !v_trk.samples.is_empty()
        && !a_trk.samples.is_empty()
    {
        let movie_timescale = 1000u32;
        let v_dur_s = v_trk
            .samples
            .iter()
            .map(|s| u64::from(s.duration.max(1)))
            .sum::<u64>() as f64
            / f64::from(v_trk.timescale.max(1));
        let a_dur_s = a_trk
            .samples
            .iter()
            .map(|s| u64::from(s.duration.max(1)))
            .sum::<u64>() as f64
            / f64::from(a_trk.timescale.max(1));
        let dur_ms = (v_dur_s.max(a_dur_s) * 1000.0)
            .round()
            .clamp(1.0, f64::from(u32::MAX)) as u32;

        let v_bytes: u64 = v_trk.samples.iter().map(|s| u64::from(s.size)).sum();
        let a_bytes: u64 = a_trk.samples.iter().map(|s| u64::from(s.size)).sum();
        let total_payload = v_bytes.saturating_add(a_bytes);
        if total_payload < u64::from(u32::MAX) - 16 * 1024 * 1024 {
            let mut ftyp_body = Vec::with_capacity(28);
            ftyp_body.extend_from_slice(b"isom");
            ftyp_body.extend_from_slice(&512u32.to_be_bytes());
            for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
                ftyp_body.extend_from_slice(brand);
            }
            let ftyp = wrap_box(b"ftyp", &ftyp_body);

            if let (Some(d_v), Some(d_a)) = (
                rebuild_defrag_trak(v_trk, 1, movie_timescale, 0),
                rebuild_defrag_trak(a_trk, 2, movie_timescale, 0),
            ) {
                let mvhd = build_mvhd(movie_timescale, dur_ms, 3);
                let dummy_moov_len = 8 + mvhd.len() + d_v.len() + d_a.len();
                let mdat_hdr_len = 8u64;
                let v_chunk_off = ftyp.len() as u64 + dummy_moov_len as u64 + mdat_hdr_len;
                let a_chunk_off = v_chunk_off + v_bytes;

                if let (Some(real_v), Some(real_a)) = (
                    rebuild_defrag_trak(v_trk, 1, movie_timescale, v_chunk_off),
                    rebuild_defrag_trak(a_trk, 2, movie_timescale, a_chunk_off),
                ) {
                    let mut moov_body = mvhd;
                    moov_body.extend_from_slice(&real_v);
                    moov_body.extend_from_slice(&real_a);
                    let moov = wrap_box(b"moov", &moov_body);

                    out.write_all(&ftyp)?;
                    out.write_all(&moov)?;
                    let mdat_total = (total_payload + 8) as u32;
                    out.write_all(&mdat_total.to_be_bytes())?;
                    out.write_all(b"mdat")?;

                    stream_track_samples(video, out, &v_trk.samples)?;
                    stream_track_samples(audio, out, &a_trk.samples)?;
                    return Ok(true);
                }
            }
        }
    }

    video.seek(SeekFrom::Start(0))?;
    audio.seek(SeekFrom::Start(0))?;
    let Some(mut v) = load_fmp4_track(video)? else {
        return Ok(false);
    };
    let Some(mut a) = load_fmp4_track(audio)? else {
        return Ok(false);
    };
    let duration_secs = v.duration_secs.max(a.duration_secs);
    let dur_ms = (duration_secs * 1000.0)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32;

    v.trak = clean_fmp4_trak_for_mux(&v.trak, 1, dur_ms, v.timescale);
    a.trak = clean_fmp4_trak_for_mux(&a.trak, 2, dur_ms, a.timescale);

    let patch_trex = |src: Option<Vec<u8>>, id: u32| -> Vec<u8> {
        match src {
            Some(mut t) if t.len() >= 16 => {
                t[12..16].copy_from_slice(&id.to_be_bytes());
                t
            }
            _ => build_trex(id),
        }
    };
    let v_trex = patch_trex(v.trex.take(), 1);
    let a_trex = patch_trex(a.trex.take(), 2);

    let mut ftyp_body = Vec::new();
    ftyp_body.extend_from_slice(b"isom");
    ftyp_body.extend_from_slice(&512u32.to_be_bytes());
    for brand in [b"isom", b"iso2", b"iso6", b"avc1", b"mp41", b"dash"] {
        ftyp_body.extend_from_slice(brand);
    }
    let ftyp = wrap_box(b"ftyp", &ftyp_body);

    let mut mvex_body = Vec::new();
    if dur_ms > 0 {
        let mut mehd = vec![0u8; 8];
        mehd[4..8].copy_from_slice(&dur_ms.to_be_bytes());
        mvex_body.extend_from_slice(&wrap_box(b"mehd", &mehd));
    }
    mvex_body.extend_from_slice(&v_trex);
    mvex_body.extend_from_slice(&a_trex);

    let mut moov_body = build_mvhd(1000, dur_ms, 3);
    moov_body.extend_from_slice(&v.trak);
    moov_body.extend_from_slice(&a.trak);
    moov_body.extend_from_slice(&wrap_box(b"mvex", &mvex_body));
    let moov = wrap_box(b"moov", &moov_body);

    // (decode seconds, is_video, moof_off, moof_len, mdat_off, mdat_len)
    let mut order: Vec<(f64, bool, u64, u64, u64, u64)> = Vec::new();
    for &(t, mo, ml, do_, dl) in &v.fragments {
        order.push((t as f64 / f64::from(v.timescale), true, mo, ml, do_, dl));
    }
    for &(t, mo, ml, do_, dl) in &a.fragments {
        order.push((t as f64 / f64::from(a.timescale), false, mo, ml, do_, dl));
    }
    order.sort_by(|x, y| x.0.total_cmp(&y.0).then(y.1.cmp(&x.1)));

    out.write_all(&ftyp)?;
    out.write_all(&moov)?;
    for (seq, (_, is_video, mo, ml, do_, dl)) in order.into_iter().enumerate() {
        let mut moof = vec![0u8; ml as usize];
        let track_id = if is_video { 1 } else { 2 };
        if is_video {
            video.seek(SeekFrom::Start(mo))?;
            video.read_exact(&mut moof)?;
        } else {
            audio.seek(SeekFrom::Start(mo))?;
            audio.read_exact(&mut moof)?;
        }
        patch_moof(&mut moof, (seq + 1) as u32, track_id);
        out.write_all(&moof)?;
        if is_video {
            video.seek(SeekFrom::Start(do_))?;
            copy_chunked(video, out, dl, 256 * 1024)?;
        } else {
            audio.seek(SeekFrom::Start(do_))?;
            copy_chunked(audio, out, dl, 256 * 1024)?;
        }
    }
    Ok(true)
}

// ----------------------------------------------------------------------------
// WebM / Matroska
// ----------------------------------------------------------------------------

const ID_EBML: u32 = 0x1A45_DFA3;
const ID_SEGMENT: u32 = 0x1853_8067;
const ID_INFO: u32 = 0x1549_A966;
const ID_TRACKS: u32 = 0x1654_AE6B;
const ID_CLUSTER: u32 = 0x1F43_B675;
const ID_TRACK_ENTRY: u32 = 0xAE;
const ID_TRACK_NUMBER: u32 = 0xD7;
const ID_TRACK_UID: u32 = 0x73C5;
const ID_TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const ID_DURATION: u32 = 0x4489;
const ID_CLUSTER_TS: u32 = 0xE7;
const ID_SIMPLE_BLOCK: u32 = 0xA3;
const ID_BLOCK_GROUP: u32 = 0xA0;
const ID_BLOCK: u32 = 0xA1;

/// Reads an EBML element ID (marker bits kept). Returns (id, len).
fn parse_id(b: &[u8]) -> Option<(u32, usize)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 4 || b.len() < len {
        return None;
    }
    let mut id = 0u32;
    for &x in &b[..len] {
        id = (id << 8) | u32::from(x);
    }
    Some((id, len))
}

/// Reads an EBML size VINT. Returns (size, len, unknown).
fn parse_size(b: &[u8]) -> Option<(u64, usize, bool)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || b.len() < len {
        return None;
    }
    let mask = if len == 8 { 0 } else { 0xFFu8 >> len };
    let mut v = u64::from(first & mask);
    let mut all_ones = (first & mask) == mask;
    for &x in &b[1..len] {
        v = (v << 8) | u64::from(x);
        all_ones &= x == 0xFF;
    }
    Some((v, len, all_ones))
}

fn vint_size(size: u64) -> Vec<u8> {
    for len in 1..=8usize {
        let max = (1u64 << (7 * len)) - 2;
        if size <= max {
            let mut out = vec![0u8; len];
            let mut v = size;
            for i in (0..len).rev() {
                out[i] = (v & 0xFF) as u8;
                v >>= 8;
            }
            out[0] |= 0x80 >> (len - 1);
            return out;
        }
    }
    vec![0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE]
}

fn id_bytes(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let skip = bytes.iter().position(|&b| b != 0).unwrap_or(3);
    bytes[skip..].to_vec()
}

fn elem(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend_from_slice(&vint_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn uint_elem(id: u32, v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let skip = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    elem(id, &bytes[skip..])
}

/// Iterates `(id, payload_start, payload_end)` of children inside `b`.
fn ebml_children(b: &[u8]) -> Option<Vec<(u32, usize, usize)>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < b.len() {
        let (id, il) = parse_id(&b[pos..])?;
        let (size, sl, unknown) = parse_size(&b[pos + il..])?;
        if unknown {
            return None;
        }
        let start = pos + il + sl;
        let end = start.checked_add(size as usize)?;
        if end > b.len() {
            return None;
        }
        out.push((id, start, end));
        pos = end;
    }
    Some(out)
}

fn read_uint(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |acc, &x| (acc << 8) | u64::from(x))
}

fn read_float(b: &[u8]) -> Option<f64> {
    match b.len() {
        4 => Some(f64::from(f32::from_be_bytes(b.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(b.try_into().ok()?)),
        _ => None,
    }
}

struct WebmTrack {
    timestamp_scale: u64,
    duration_ticks: f64,
    track_entry: Vec<u8>,
    clusters: Vec<(u64, u64, u64)>, // timestamp, payload_offset, payload_len
}

fn read_element_header<R: Read + Seek>(
    r: &mut R,
    pos: u64,
    file_len: u64,
) -> RuviResult<Option<(u32, u64, u64, bool)>> {
    let avail = (file_len - pos).min(12) as usize;
    if avail < 2 {
        return Ok(None);
    }
    let mut hdr = [0u8; 12];
    r.seek(SeekFrom::Start(pos))?;
    r.read_exact(&mut hdr[..avail])?;
    let Some((id, il)) = parse_id(&hdr[..avail]) else {
        return Ok(None);
    };
    let Some((size, sl, unknown)) = parse_size(&hdr[il..avail]) else {
        return Ok(None);
    };
    Ok(Some((id, (il + sl) as u64, size, unknown)))
}

fn load_webm_track<R: Read + Seek>(r: &mut R) -> RuviResult<Option<WebmTrack>> {
    let file_len = r.seek(SeekFrom::End(0))?;
    let Some((id, hl, size, unknown)) = read_element_header(r, 0, file_len)? else {
        return Ok(None);
    };
    if id != ID_EBML || unknown {
        return Ok(None);
    }
    let seg_pos = hl + size;
    let Some((sid, shl, ssize, sunknown)) = read_element_header(r, seg_pos, file_len)? else {
        return Ok(None);
    };
    if sid != ID_SEGMENT {
        return Ok(None);
    }
    let seg_start = seg_pos + shl;
    let seg_end = if sunknown {
        file_len
    } else {
        (seg_start + ssize).min(file_len)
    };

    let mut timestamp_scale = 1_000_000u64;
    let mut duration_ticks = 0.0f64;
    let mut track_entry: Option<Vec<u8>> = None;
    let mut clusters = Vec::new();

    let mut pos = seg_start;
    while pos < seg_end {
        let Some((cid, chl, csize, cunknown)) = read_element_header(r, pos, file_len)? else {
            break;
        };
        if cunknown {
            return Ok(None);
        }
        let payload_start = pos + chl;
        let payload_end = payload_start + csize;
        if payload_end > file_len {
            return Ok(None);
        }
        match cid {
            ID_INFO | ID_TRACKS => {
                if csize > 16 * 1024 * 1024 {
                    return Ok(None);
                }
                let mut buf = vec![0u8; csize as usize];
                r.seek(SeekFrom::Start(payload_start))?;
                r.read_exact(&mut buf)?;
                let Some(children) = ebml_children(&buf) else {
                    return Ok(None);
                };
                for (eid, s, e) in children {
                    match (cid, eid) {
                        (ID_INFO, ID_TIMESTAMP_SCALE) => timestamp_scale = read_uint(&buf[s..e]),
                        (ID_INFO, ID_DURATION) => {
                            duration_ticks = read_float(&buf[s..e]).unwrap_or(0.0)
                        }
                        (ID_TRACKS, ID_TRACK_ENTRY) if track_entry.is_none() => {
                            track_entry = Some(buf[s..e].to_vec())
                        }
                        (ID_TRACKS, ID_TRACK_ENTRY) => return Ok(None), // multi-track input
                        _ => {}
                    }
                }
            }
            ID_CLUSTER => {
                let peek_len = csize.min(32) as usize;
                let mut peek = vec![0u8; peek_len];
                r.seek(SeekFrom::Start(payload_start))?;
                r.read_exact(&mut peek)?;
                let mut ts = None;
                if let Some((tid, til)) = parse_id(&peek)
                    && tid == ID_CLUSTER_TS
                    && let Some((tsz, tsl, _)) = parse_size(&peek[til..])
                {
                    let s = til + tsl;
                    if let Some(bytes) = peek.get(s..s + tsz as usize) {
                        ts = Some(read_uint(bytes));
                    }
                }
                let Some(ts) = ts else {
                    return Ok(None);
                };
                clusters.push((ts, payload_start, csize));
            }
            _ => {}
        }
        pos = payload_end;
    }

    let Some(track_entry) = track_entry else {
        return Ok(None);
    };
    if clusters.is_empty() || timestamp_scale == 0 {
        return Ok(None);
    }
    Ok(Some(WebmTrack {
        timestamp_scale,
        duration_ticks,
        track_entry,
        clusters,
    }))
}

fn patch_track_entry(entry: &[u8], num: u64) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(entry.len() + 8);
    out.extend_from_slice(&uint_elem(ID_TRACK_NUMBER, num));
    out.extend_from_slice(&uint_elem(ID_TRACK_UID, num));
    for (id, s, e) in ebml_children(entry)? {
        if id == ID_TRACK_NUMBER || id == ID_TRACK_UID {
            continue;
        }
        out.extend_from_slice(&elem(id, &entry[s..e]));
    }
    Some(elem(ID_TRACK_ENTRY, &out))
}

fn retrack_block(payload: &[u8], track: u8) -> Option<Vec<u8>> {
    let first = *payload.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || payload.len() < len {
        return None;
    }
    let mut out = Vec::with_capacity(payload.len());
    out.push(0x80 | track);
    out.extend_from_slice(&payload[len..]);
    Some(out)
}

fn rewrite_cluster(payload: &[u8], track: u8) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(payload.len() + 16);
    for (id, s, e) in ebml_children(payload)? {
        match id {
            ID_SIMPLE_BLOCK => {
                out.extend_from_slice(&elem(id, &retrack_block(&payload[s..e], track)?))
            }
            ID_BLOCK_GROUP => {
                let group = &payload[s..e];
                let mut g = Vec::with_capacity(group.len());
                for (gid, gs, ge) in ebml_children(group)? {
                    if gid == ID_BLOCK {
                        g.extend_from_slice(&elem(gid, &retrack_block(&group[gs..ge], track)?));
                    } else {
                        g.extend_from_slice(&elem(gid, &group[gs..ge]));
                    }
                }
                out.extend_from_slice(&elem(id, &g));
            }
            _ => out.extend_from_slice(&elem(id, &payload[s..e])),
        }
    }
    Some(elem(ID_CLUSTER, &out))
}

/// Losslessly interleaves two single-track WebM inputs (e.g. VP9/AV1 + Opus)
/// into one WebM with video as track 1 and audio as track 2.
pub fn try_mux_webm<R1: Read + Seek, R2: Read + Seek, W: Write>(
    video: &mut R1,
    audio: &mut R2,
    out: &mut W,
) -> RuviResult<bool> {
    let Some(v) = load_webm_track(video)? else {
        return Ok(false);
    };
    let Some(a) = load_webm_track(audio)? else {
        return Ok(false);
    };
    if v.timestamp_scale != a.timestamp_scale {
        return Ok(false);
    }
    let (Some(v_entry), Some(a_entry)) = (
        patch_track_entry(&v.track_entry, 1),
        patch_track_entry(&a.track_entry, 2),
    ) else {
        return Ok(false);
    };

    let mut header = Vec::new();
    header.extend_from_slice(&uint_elem(0x4286, 1));
    header.extend_from_slice(&uint_elem(0x42F7, 1));
    header.extend_from_slice(&uint_elem(0x42F2, 4));
    header.extend_from_slice(&uint_elem(0x42F3, 8));
    header.extend_from_slice(&elem(0x4282, b"webm"));
    header.extend_from_slice(&uint_elem(0x4287, 4));
    header.extend_from_slice(&uint_elem(0x4285, 2));

    let mut info = Vec::new();
    info.extend_from_slice(&uint_elem(ID_TIMESTAMP_SCALE, v.timestamp_scale));
    let last_cluster_ticks = v
        .clusters
        .last()
        .map(|c| c.0)
        .unwrap_or(0)
        .max(a.clusters.last().map(|c| c.0).unwrap_or(0));
    let dur = v
        .duration_ticks
        .max(a.duration_ticks)
        .max(if last_cluster_ticks > 0 {
            (last_cluster_ticks + 2000) as f64
        } else {
            0.0
        });
    if dur > 0.0 {
        info.extend_from_slice(&elem(ID_DURATION, &dur.to_be_bytes()));
    }
    info.extend_from_slice(&elem(0x4D80, b"ruvic-pure-rust"));
    info.extend_from_slice(&elem(0x5741, b"ruvic-pure-rust"));

    let mut tracks = v_entry;
    tracks.extend_from_slice(&a_entry);

    out.write_all(&elem(ID_EBML, &header))?;
    out.write_all(&id_bytes(ID_SEGMENT))?;
    out.write_all(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])?;
    out.write_all(&elem(ID_INFO, &info))?;
    out.write_all(&elem(ID_TRACKS, &tracks))?;

    let (mut vi, mut ai) = (0usize, 0usize);
    while vi < v.clusters.len() || ai < a.clusters.len() {
        let take_video = match (v.clusters.get(vi), a.clusters.get(ai)) {
            (Some(vc), Some(ac)) => vc.0 <= ac.0,
            (Some(_), None) => true,
            _ => false,
        };
        let (cluster, track) = if take_video {
            vi += 1;
            (v.clusters[vi - 1], 1u8)
        } else {
            ai += 1;
            (a.clusters[ai - 1], 2u8)
        };
        let mut buf = vec![0u8; cluster.2 as usize];
        if take_video {
            video.seek(SeekFrom::Start(cluster.1))?;
            video.read_exact(&mut buf)?;
        } else {
            audio.seek(SeekFrom::Start(cluster.1))?;
            audio.read_exact(&mut buf)?;
        }
        let Some(rewritten) = rewrite_cluster(&buf, track) else {
            return Err(crate::error::RuviError::Integrity(
                "malformed WebM cluster during dual-stream mux".to_string(),
            ));
        };
        out.write_all(&rewritten)?;
    }
    Ok(true)
}

fn build_itunes_text_atom(fourcc: &[u8; 4], text: &str) -> Vec<u8> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut data_body = Vec::with_capacity(8 + trimmed.len());
    data_body.extend_from_slice(&1u32.to_be_bytes()); // type 1 = UTF-8
    data_body.extend_from_slice(&0u32.to_be_bytes()); // locale 0
    data_body.extend_from_slice(trimmed.as_bytes());
    let data_box = wrap_box(b"data", &data_body);
    wrap_box(fourcc, &data_box)
}

fn build_itunes_covr_atom(img: &[u8]) -> Vec<u8> {
    if img.len() < 16 {
        return Vec::new();
    }
    let data_type: u32 = if img.starts_with(b"\x89PNG") { 14 } else { 13 }; // 13 = JPEG, 14 = PNG
    let mut data_body = Vec::with_capacity(8 + img.len());
    data_body.extend_from_slice(&data_type.to_be_bytes());
    data_body.extend_from_slice(&0u32.to_be_bytes());
    data_body.extend_from_slice(img);
    let data_box = wrap_box(b"data", &data_body);
    wrap_box(b"covr", &data_box)
}

/// Builds a standard ISO-BMFF / iTunes `udta` -> `meta` -> `ilst` box containing
/// Title (`©nam`), Artist (`©ART`), Album Artist (`aART`), Album (`©alb`), Year (`©day`),
/// Encoder (`©too`), Comment (`©cmt`), and Cover Art (`covr`).
pub fn build_mp4_udta_metadata_box(
    title: &str,
    artist: &str,
    album: &str,
    year: &str,
    comment: &str,
    cover_jpeg: Option<&[u8]>,
) -> Vec<u8> {
    let mut ilst_body = Vec::new();
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"\xA9nam", title));
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"\xA9ART", artist));
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"aART", artist));
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"\xA9alb", album));
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"\xA9day", year));
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"\xA9gen", "Music"));
    ilst_body.extend_from_slice(&build_itunes_text_atom(b"\xA9cmt", comment));
    ilst_body.extend_from_slice(&build_itunes_text_atom(
        b"\xA9too",
        "RuviTools Pure-Rust Engine",
    ));
    if let Some(img) = cover_jpeg {
        ilst_body.extend_from_slice(&build_itunes_covr_atom(img));
    }
    if ilst_body.is_empty() {
        return Vec::new();
    }
    let mut hdlr_body = vec![0u8; 25];
    hdlr_body[8..12].copy_from_slice(b"mdir");
    hdlr_body[12..16].copy_from_slice(b"appl");
    let hdlr_box = wrap_box(b"hdlr", &hdlr_body);
    let ilst_box = wrap_box(b"ilst", &ilst_body);

    let mut meta_body = Vec::with_capacity(4 + hdlr_box.len() + ilst_box.len());
    meta_body.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    meta_body.extend_from_slice(&hdlr_box);
    meta_body.extend_from_slice(&ilst_box);
    let meta_box = wrap_box(b"meta", &meta_body);
    wrap_box(b"udta", &meta_box)
}

/// Injects `udta` -> `meta` -> `ilst` iTunes metadata and cover art into a fragmented or standard Faststart MP4 / M4A file's `moov` box.
/// For standard Faststart MP4s (`moov` before `mdat`), shifts `stco`/`co64` chunk offsets by the exact `moov` size delta.
pub fn inject_fmp4_itunes_metadata_file(
    mp4_path: &std::path::Path,
    title: &str,
    artist: &str,
    album: &str,
    year: &str,
    comment: &str,
    cover_jpeg: Option<&[u8]>,
) -> RuviResult<bool> {
    use std::fs::File;
    use std::io::BufWriter;

    let udta = build_mp4_udta_metadata_box(title, artist, album, year, comment, cover_jpeg);
    if udta.is_empty() {
        return Ok(false);
    }
    let mut src = File::open(mp4_path)?;
    let Some(boxes) = scan_top_level_boxes(&mut src)? else {
        return Ok(false);
    };
    let has_moof = boxes.iter().any(|b| &b.kind == b"moof");
    let has_mdat = boxes.iter().any(|b| &b.kind == b"mdat");
    let has_sidx = boxes.iter().any(|b| &b.kind == b"sidx");
    if !has_moof && !has_mdat {
        return Ok(false);
    }
    let Some(moov_box) = boxes.iter().find(|b| &b.kind == b"moov").copied() else {
        return Ok(false);
    };

    let mut moov_raw = vec![0u8; moov_box.size as usize];
    src.seek(SeekFrom::Start(moov_box.offset))?;
    src.read_exact(&mut moov_raw)?;
    let moov_body = &moov_raw[moov_box.header as usize..];

    let mut new_moov_body = Vec::with_capacity(moov_body.len() + udta.len());
    for (kind, s, e) in child_boxes(moov_body) {
        if &kind == b"udta" {
            continue;
        }
        new_moov_body.extend_from_slice(&moov_body[s..e]);
    }
    new_moov_body.extend_from_slice(&udta);
    let mut new_moov = wrap_box(b"moov", &new_moov_body);

    if !has_moof {
        let first_mdat_off = boxes
            .iter()
            .find(|b| &b.kind == b"mdat")
            .map(|b| b.offset)
            .unwrap_or(u64::MAX);
        if moov_box.offset < first_mdat_off {
            let mut shift = new_moov.len() as i64 - moov_box.size as i64;
            for (b_idx, b) in boxes.iter().enumerate() {
                if &b.kind == b"sidx" && has_sidx && b.offset < first_mdat_off {
                    shift -= b.size as i64;
                }
                let _ = b_idx;
            }
            if shift != 0 {
                new_moov =
                    crate::backend::native::mp4_atoms::shift_moov_chunk_offsets(&new_moov, shift)?;
            }
        }
    }

    let tmp = mp4_path.with_extension("meta.tmp");
    {
        let mut out = BufWriter::with_capacity(256 * 1024, File::create(&tmp)?);
        for b in boxes {
            if &b.kind == b"moov" {
                out.write_all(&new_moov)?;
            } else if &b.kind == b"sidx" && has_sidx {
                // Omit sidx when expanding moov so players use moof/stbl directly.
                continue;
            } else {
                src.seek(SeekFrom::Start(b.offset))?;
                copy_chunked(&mut src, &mut out, b.size, 256 * 1024)?;
            }
        }
        out.flush()?;
    }
    drop(src);
    std::fs::rename(&tmp, mp4_path)?;
    Ok(true)
}

fn tkhd_track_id(trak_body: &[u8]) -> Option<u32> {
    let (s, e) = find_child(trak_body, b"tkhd")?;
    let tkhd = &trak_body[s + 8..e];
    match tkhd.first()? {
        1 => read_u32(tkhd, 20),
        _ => read_u32(tkhd, 12),
    }
}

fn tfhd_track_id(moof: &[u8]) -> Option<u32> {
    let body = &moof[8.min(moof.len())..];
    let (ts, te) = find_child(body, b"traf")?;
    let traf = &body[ts + 8..te];
    let (hs, he) = find_child(traf, b"tfhd")?;
    let tfhd = &traf[hs + 8..he];
    read_u32(tfhd, 4)
}

fn patch_moof_seq_and_tfdt(moof: &mut [u8], seq: u32, new_tfdt: u64) {
    if moof.len() < 8 {
        return;
    }
    let children = child_boxes(&moof[8..]);
    for (kind, s, e) in children {
        let abs = 8 + s;
        if &kind == b"mfhd" && abs + 16 <= 8 + e {
            moof[abs + 12..abs + 16].copy_from_slice(&seq.to_be_bytes());
        } else if &kind == b"traf" {
            let traf_body_start = abs + 8;
            let inner = child_boxes(&moof[traf_body_start..8 + e]);
            for (ik, is, ie) in inner {
                let iabs = traf_body_start + is;
                if &ik == b"tfdt" {
                    let ver = moof.get(iabs + 8).copied().unwrap_or(0);
                    if ver == 1 && iabs + 20 <= traf_body_start + ie {
                        moof[iabs + 12..iabs + 20].copy_from_slice(&new_tfdt.to_be_bytes());
                    } else if ver == 0 && iabs + 16 <= traf_body_start + ie {
                        let v32 = u32::try_from(new_tfdt).unwrap_or(u32::MAX);
                        moof[iabs + 12..iabs + 16].copy_from_slice(&v32.to_be_bytes());
                    }
                }
            }
        }
    }
}

fn collect_nonfrag_tracks<R: Read + Seek>(r: &mut R) -> RuviResult<Option<Vec<DefragTrack>>> {
    let file_len = r.seek(SeekFrom::End(0))?;
    let Some(boxes) = scan_top_level_boxes(r)? else {
        return Ok(None);
    };
    if boxes.iter().any(|b| &b.kind == b"moof") {
        return Ok(None);
    }
    let Some(moov_box) = boxes.iter().find(|b| &b.kind == b"moov").copied() else {
        return Ok(None);
    };
    if moov_box.size > 64 * 1024 * 1024 {
        return Ok(None);
    }
    let mut moov = vec![0u8; moov_box.size as usize];
    r.seek(SeekFrom::Start(moov_box.offset))?;
    r.read_exact(&mut moov)?;
    let moov_body = &moov[moov_box.header as usize..];

    let mut duration_seconds = None;
    let mut streams = Vec::new();
    let mut tables = Vec::new();
    crate::backend::native::mp4_atoms::parse_moov(
        moov_body,
        &mut duration_seconds,
        &mut streams,
        &mut tables,
    )?;

    let raw_traks: Vec<(usize, usize)> = child_boxes(moov_body)
        .into_iter()
        .filter(|(k, _, _)| k == b"trak")
        .map(|(_, s, e)| (s, e))
        .collect();
    if raw_traks.len() != tables.len() || tables.is_empty() {
        return Ok(None);
    }

    let mut out = Vec::with_capacity(tables.len());
    for (idx, tbl) in tables.into_iter().enumerate() {
        let ranges = tbl.compute_sample_ranges();
        if ranges.len() <= 1
            || ranges
                .iter()
                .any(|&(off, sz)| sz == 0 || off.saturating_add(u64::from(sz)) > file_len)
        {
            return Ok(None);
        }
        let (ts_s, ts_e) = raw_traks[idx];
        let orig_trak = moov_body[ts_s..ts_e].to_vec();
        let tbody = &orig_trak[8..];
        let timescale = mdhd_timescale(tbody).unwrap_or(1000).max(1);
        let is_audio = &tbl.handler_type == b"soun";

        // Expand stts run-length table
        let mut durations = Vec::with_capacity(ranges.len());
        for (cnt, delta) in &tbl.time_to_sample {
            for _ in 0..(*cnt as usize) {
                if durations.len() >= ranges.len() {
                    break;
                }
                durations.push((*delta).max(1));
            }
        }
        let default_dur = durations
            .last()
            .copied()
            .unwrap_or(if is_audio { 1024 } else { 512 });
        while durations.len() < ranges.len() {
            durations.push(default_dur);
        }

        // Expand optional ctts
        let mut cts_offsets = vec![0i32; ranges.len()];
        let mut has_cts = false;
        if let Some((ms, me)) = find_child(tbody, b"mdia")
            && let Some((ns, ne)) = find_child(&tbody[ms + 8..me], b"minf")
            && let Some((bs, be)) = find_child(&tbody[ms + 8..me][ns + 8..ne], b"stbl")
        {
            let stbl = &tbody[ms + 8..me][ns + 8..ne][bs + 8..be];
            if let Some((cs, ce)) = find_child(stbl, b"ctts") {
                let cbody = &stbl[cs + 8..ce];
                if cbody.len() >= 8 {
                    let count = read_u32(cbody, 4).unwrap_or(0) as usize;
                    let mut cpos = 8usize;
                    let mut s_i = 0usize;
                    for _ in 0..count {
                        if cpos + 8 > cbody.len() {
                            break;
                        }
                        let cnt = read_u32(cbody, cpos).unwrap_or(0) as usize;
                        let off = read_u32(cbody, cpos + 4).unwrap_or(0) as i32;
                        if off != 0 {
                            has_cts = true;
                        }
                        for _ in 0..cnt {
                            if s_i < cts_offsets.len() {
                                cts_offsets[s_i] = off;
                                s_i += 1;
                            }
                        }
                        cpos += 8;
                    }
                }
            }
        }

        let mut samples = Vec::with_capacity(ranges.len());
        let mut dts = 0u64;
        let has_real_keyframes = tbl.sync_samples.as_ref().is_some_and(|s| s.len() > 1);
        for (s_i, (file_offset, size)) in ranges.into_iter().enumerate() {
            let dur = durations[s_i];
            let is_sync = if is_audio || !has_real_keyframes {
                true
            } else if let Some(ref syncs) = tbl.sync_samples {
                syncs.binary_search(&((s_i + 1) as u32)).is_ok()
            } else {
                true
            };
            samples.push(DefragSample {
                file_offset,
                size,
                duration: dur,
                cts_offset: cts_offsets[s_i],
                is_sync,
                dts,
            });
            dts = dts.saturating_add(u64::from(dur));
        }

        out.push(DefragTrack {
            orig_trak,
            track_id: tbl.track_id,
            timescale,
            is_audio,
            has_cts,
            samples,
        });
    }
    Ok(Some(out))
}

/// Losslessly trims a single-track or multi-track fragmented or standard MP4 (`.mp4` / `.m4a`)
/// with sub-millisecond AAC frame accuracy (~23.2 ms) and video keyframe alignment,
/// producing a standard non-fragmented Faststart MP4 so VLC and Windows Media Player play
/// video and audio in 100% synchronization from 00:00.
pub fn try_trim_fmp4<R: Read + Seek, W: Write>(
    src: &mut R,
    out: &mut W,
    start_sec: f64,
    duration_sec: f64,
) -> RuviResult<bool> {
    src.seek(SeekFrom::Start(0))?;
    let defrag_opt = match collect_defrag_tracks(src)? {
        Some(t) => Some(t),
        None => {
            src.seek(SeekFrom::Start(0))?;
            collect_nonfrag_tracks(src)?
        }
    };

    if let Some(mut tracks) = defrag_opt
        && !tracks.is_empty()
    {
        let start_s = start_sec.max(0.0);
        let end_s = (start_s + duration_sec.max(0.1)).max(start_s + 0.1);

        // Anchor clip start on the last video keyframe at or before start_s
        let actual_start_s = if let Some(v_trk) = tracks.iter().find(|t| !t.is_audio) {
            let v_ts = f64::from(v_trk.timescale.max(1));
            v_trk
                .samples
                .iter()
                .filter(|s| s.is_sync && (s.dts as f64 / v_ts) <= start_s + 0.05)
                .map(|s| s.dts as f64 / v_ts)
                .next_back()
                .or_else(|| v_trk.samples.first().map(|s| s.dts as f64 / v_ts))
                .unwrap_or(0.0)
        } else {
            let a_trk = &tracks[0];
            let a_ts = f64::from(a_trk.timescale.max(1));
            a_trk
                .samples
                .iter()
                .filter(|s| (s.dts as f64 / a_ts) <= start_s + 0.02)
                .map(|s| s.dts as f64 / a_ts)
                .next_back()
                .unwrap_or(0.0)
        };

        let actual_end_s = (actual_start_s + (end_s - start_s).max(0.1)).max(end_s);

        // Slice each track's individual samples to [actual_start_s, actual_end_s]
        for trk in &mut tracks {
            let ts = f64::from(trk.timescale.max(1));
            let first_idx = if trk.is_audio {
                // Pick the exact AAC frame (~23.2ms) whose presentation interval covers or starts at actual_start_s
                trk.samples
                    .iter()
                    .position(|s| {
                        ((s.dts + u64::from(s.duration / 2)) as f64 / ts) >= actual_start_s
                    })
                    .unwrap_or(0)
            } else {
                trk.samples
                    .iter()
                    .position(|s| s.is_sync && (s.dts as f64 / ts) + 1e-4 >= actual_start_s)
                    .or_else(|| {
                        trk.samples
                            .iter()
                            .position(|s| (s.dts as f64 / ts) + 1e-4 >= actual_start_s)
                    })
                    .unwrap_or(0)
            };

            let end_idx = trk
                .samples
                .iter()
                .rposition(|s| (s.dts as f64 / ts) < actual_end_s + 0.02)
                .map(|i| (i + 1).min(trk.samples.len()))
                .unwrap_or(trk.samples.len())
                .max((first_idx + 1).min(trk.samples.len()));

            let mut sliced = trk.samples[first_idx..end_idx].to_vec();
            let base_dts = sliced.first().map(|s| s.dts).unwrap_or(0);
            for s in &mut sliced {
                s.dts = s.dts.saturating_sub(base_dts);
            }
            trk.samples = sliced;
        }

        if tracks.iter().all(|t| !t.samples.is_empty()) {
            let movie_timescale = 1000u32;
            let max_dur_s = tracks
                .iter()
                .map(|t| {
                    t.samples
                        .iter()
                        .map(|s| u64::from(s.duration.max(1)))
                        .sum::<u64>() as f64
                        / f64::from(t.timescale.max(1))
                })
                .fold(0.5f64, f64::max);
            let dur_ms = (max_dur_s * 1000.0).round().clamp(1.0, f64::from(u32::MAX)) as u32;

            let total_payload: u64 = tracks
                .iter()
                .map(|t| t.samples.iter().map(|s| u64::from(s.size)).sum::<u64>())
                .sum();

            if total_payload < u64::from(u32::MAX) - 16 * 1024 * 1024 {
                let mut ftyp_body = Vec::with_capacity(28);
                ftyp_body.extend_from_slice(b"isom");
                ftyp_body.extend_from_slice(&512u32.to_be_bytes());
                for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
                    ftyp_body.extend_from_slice(brand);
                }
                let ftyp = wrap_box(b"ftyp", &ftyp_body);

                let mut dummy_traks = Vec::with_capacity(tracks.len());
                let mut ok = true;
                for (i, t) in tracks.iter().enumerate() {
                    if let Some(dt) = rebuild_defrag_trak(t, (i + 1) as u32, movie_timescale, 0) {
                        dummy_traks.push(dt);
                    } else {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    let next_tid = (tracks.len() + 1) as u32;
                    let mvhd = build_mvhd(movie_timescale, dur_ms, next_tid);
                    let dummy_moov_len =
                        8 + mvhd.len() + dummy_traks.iter().map(Vec::len).sum::<usize>();
                    let mut running_chunk_off = ftyp.len() as u64 + dummy_moov_len as u64 + 8u64;

                    let mut real_traks = Vec::with_capacity(tracks.len());
                    for (i, t) in tracks.iter().enumerate() {
                        let t_bytes: u64 = t.samples.iter().map(|s| u64::from(s.size)).sum();
                        if let Some(rt) = rebuild_defrag_trak(
                            t,
                            (i + 1) as u32,
                            movie_timescale,
                            running_chunk_off,
                        ) {
                            real_traks.push(rt);
                            running_chunk_off += t_bytes;
                        } else {
                            ok = false;
                            break;
                        }
                    }

                    if ok {
                        let mut moov_body = mvhd;
                        for rt in real_traks {
                            moov_body.extend_from_slice(&rt);
                        }
                        let moov = wrap_box(b"moov", &moov_body);

                        out.write_all(&ftyp)?;
                        out.write_all(&moov)?;
                        let mdat_total = (total_payload + 8) as u32;
                        out.write_all(&mdat_total.to_be_bytes())?;
                        out.write_all(b"mdat")?;

                        for t in &tracks {
                            stream_track_samples(src, out, &t.samples)?;
                        }
                        return Ok(true);
                    }
                }
            }
        }
    }

    src.seek(SeekFrom::Start(0))?;
    let Some(boxes) = scan_top_level_boxes(src)? else {
        return Ok(false);
    };
    let Some(moov_box) = boxes.iter().find(|b| &b.kind == b"moov").copied() else {
        return Ok(false);
    };
    if !boxes.iter().any(|b| &b.kind == b"moof") || moov_box.size > 64 * 1024 * 1024 {
        return Ok(false);
    }

    let mut moov = vec![0u8; moov_box.size as usize];
    src.seek(SeekFrom::Start(moov_box.offset))?;
    src.read_exact(&mut moov)?;
    let moov_body = &moov[moov_box.header as usize..];

    let mut track_timescales: Vec<(u32, u32)> = Vec::new();
    for (k, s, e) in child_boxes(moov_body) {
        if &k == b"trak" {
            let tbody = &moov_body[s + 8..e];
            let tid = tkhd_track_id(tbody).unwrap_or((track_timescales.len() + 1) as u32);
            let ts = mdhd_timescale(tbody).unwrap_or(1000).max(1);
            track_timescales.push((tid, ts));
        }
    }
    if track_timescales.is_empty() {
        return Ok(false);
    }
    let primary_tid = track_timescales[0].0;

    // Collect all (track_id, decode_time, time_sec, moof_box, mdat_box)
    let mut frags: Vec<(u32, u64, f64, TopBox, TopBox)> = Vec::new();
    let mut i = 0usize;
    while i < boxes.len() {
        if &boxes[i].kind == b"moof" {
            let Some(mdat) = boxes.get(i + 1).copied().filter(|b| &b.kind == b"mdat") else {
                return Ok(false);
            };
            if boxes[i].size > 16 * 1024 * 1024 {
                return Ok(false);
            }
            let mut m = vec![0u8; boxes[i].size as usize];
            src.seek(SeekFrom::Start(boxes[i].offset))?;
            src.read_exact(&mut m)?;
            let tid = tfhd_track_id(&m).unwrap_or(primary_tid);
            let dt = tfdt_decode_time(&m);
            let ts = track_timescales
                .iter()
                .find(|(id, _)| *id == tid)
                .map(|(_, s)| *s)
                .unwrap_or(1000);
            let sec = dt as f64 / f64::from(ts);
            frags.push((tid, dt, sec, boxes[i], mdat));
            i += 2;
        } else {
            i += 1;
        }
    }
    if frags.is_empty() {
        return Ok(false);
    }

    let start_s = start_sec.max(0.0);
    let end_s = (start_s + duration_sec.max(0.1)).max(start_s + 0.1);

    // Anchor on the last primary-track (video) keyframe fragment at or before start_s
    let actual_start_s = frags
        .iter()
        .filter(|(tid, _, sec, _, _)| *tid == primary_tid && *sec <= start_s + 0.05)
        .map(|(_, _, sec, _, _)| *sec)
        .next_back()
        .or_else(|| {
            frags
                .iter()
                .filter(|(_, _, sec, _, _)| *sec <= start_s + 0.05)
                .map(|(_, _, sec, _, _)| *sec)
                .next_back()
        })
        .unwrap_or(0.0);

    // Keep for each track the last fragment at or before actual_start_s plus all fragments up to end_s
    let mut kept: Vec<(u32, u64, f64, TopBox, TopBox)> = Vec::new();
    for &(tid, _) in &track_timescales {
        let track_start_s = frags
            .iter()
            .filter(|(id, _, sec, _, _)| *id == tid && *sec <= actual_start_s + 0.05)
            .map(|(_, _, sec, _, _)| *sec)
            .next_back()
            .unwrap_or(actual_start_s);
        for f in frags.iter().copied() {
            if f.0 == tid && f.2 + 0.05 >= track_start_s && f.2 <= end_s + 0.5 {
                kept.push(f);
            }
        }
    }
    kept.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
    if kept.is_empty() {
        kept.push(frags[0]);
    }

    let clip_dur_s = (end_s - actual_start_s).max(0.5);
    let next_track_id = track_timescales
        .iter()
        .map(|(id, _)| *id)
        .max()
        .unwrap_or(1)
        + 1;

    // Write ftyp
    if let Some(ftyp) = boxes.iter().find(|b| &b.kind == b"ftyp") {
        src.seek(SeekFrom::Start(ftyp.offset))?;
        copy_chunked(src, out, ftyp.size, 64 * 1024)?;
    } else {
        out.write_all(&wrap_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2avc1mp41"))?;
    }

    // Rebuild moov with updated mvhd duration and without stale mehd
    let mut new_moov_body = Vec::with_capacity(moov_body.len() + 64);
    new_moov_body.extend_from_slice(&build_mvhd(
        1000,
        (clip_dur_s * 1000.0).round() as u32,
        next_track_id,
    ));
    for (k, s, e) in child_boxes(moov_body) {
        if &k == b"mvhd" {
            continue;
        } else if &k == b"mvex" {
            let mvex_body = &moov_body[s + 8..e];
            let mut clean_mvex = Vec::with_capacity(mvex_body.len());
            for (mk, ms, me) in child_boxes(mvex_body) {
                if &mk != b"mehd" {
                    clean_mvex.extend_from_slice(&mvex_body[ms..me]);
                }
            }
            new_moov_body.extend_from_slice(&wrap_box(b"mvex", &clean_mvex));
        } else {
            new_moov_body.extend_from_slice(&moov_body[s..e]);
        }
    }
    out.write_all(&wrap_box(b"moov", &new_moov_body))?;

    let mut base_tfdt: Vec<(u32, u64)> = Vec::new();
    for (seq_idx, (tid, dt, _, moof_b, mdat_b)) in kept.into_iter().enumerate() {
        let base = if let Some((_, b)) = base_tfdt.iter().find(|(id, _)| *id == tid) {
            *b
        } else {
            base_tfdt.push((tid, dt));
            dt
        };
        let shifted_dt = dt.saturating_sub(base);
        let mut moof_bytes = vec![0u8; moof_b.size as usize];
        src.seek(SeekFrom::Start(moof_b.offset))?;
        src.read_exact(&mut moof_bytes)?;
        patch_moof_seq_and_tfdt(&mut moof_bytes, (seq_idx + 1) as u32, shifted_dt);
        out.write_all(&moof_bytes)?;

        src.seek(SeekFrom::Start(mdat_b.offset))?;
        copy_chunked(src, out, mdat_b.size, 256 * 1024)?;
    }

    Ok(true)
}

fn rewrite_cluster_timestamp(payload: &[u8], new_ts: u64) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(payload.len() + 16);
    let mut wrote_ts = false;
    for (id, s, e) in ebml_children(payload)? {
        if id == ID_CLUSTER_TS {
            out.extend_from_slice(&uint_elem(ID_CLUSTER_TS, new_ts));
            wrote_ts = true;
        } else {
            out.extend_from_slice(&elem(id, &payload[s..e]));
        }
    }
    if !wrote_ts {
        let mut prefixed = uint_elem(ID_CLUSTER_TS, new_ts);
        prefixed.extend_from_slice(&out);
        out = prefixed;
    }
    Some(elem(ID_CLUSTER, &out))
}

/// Losslessly trims a single-track or multi-track WebM / Matroska (`.webm` / `.mkv`)
/// along real `Cluster` boundaries, anchoring on the keyframe cluster at or before `start_sec`
/// and shifting `Cluster` timestamps to start at 0 so VLC opens and plays the clip cleanly.
pub fn try_trim_webm<R: Read + Seek, W: Write>(
    src: &mut R,
    out: &mut W,
    start_sec: f64,
    duration_sec: f64,
) -> RuviResult<bool> {
    let file_len = src.seek(SeekFrom::End(0))?;
    let Some((id, hl, size, unknown)) = read_element_header(src, 0, file_len)? else {
        return Ok(false);
    };
    if id != ID_EBML || unknown || size > 4 * 1024 * 1024 {
        return Ok(false);
    }
    let mut ebml_header_payload = vec![0u8; size as usize];
    src.seek(SeekFrom::Start(hl))?;
    src.read_exact(&mut ebml_header_payload)?;

    let seg_pos = hl + size;
    let Some((sid, shl, ssize, sunknown)) = read_element_header(src, seg_pos, file_len)? else {
        return Ok(false);
    };
    if sid != ID_SEGMENT {
        return Ok(false);
    }
    let seg_start = seg_pos + shl;
    let seg_end = if sunknown {
        file_len
    } else {
        (seg_start + ssize).min(file_len)
    };

    let mut timestamp_scale = 1_000_000u64;
    let mut tracks_payload: Option<Vec<u8>> = None;
    // (ts, first_block_track, payload_start, csize)
    let mut clusters: Vec<(u64, u8, u64, u64)> = Vec::new();

    let mut pos = seg_start;
    while pos < seg_end {
        let Some((cid, chl, csize, cunknown)) = read_element_header(src, pos, file_len)? else {
            break;
        };
        if cunknown {
            return Ok(false);
        }
        let payload_start = pos + chl;
        let payload_end = payload_start + csize;
        if payload_end > file_len {
            break;
        }
        match cid {
            ID_INFO => {
                if csize <= 4 * 1024 * 1024 {
                    let mut buf = vec![0u8; csize as usize];
                    src.seek(SeekFrom::Start(payload_start))?;
                    src.read_exact(&mut buf)?;
                    if let Some(children) = ebml_children(&buf) {
                        for (eid, s, e) in children {
                            if eid == ID_TIMESTAMP_SCALE {
                                timestamp_scale = read_uint(&buf[s..e]).max(1);
                            }
                        }
                    }
                }
            }
            ID_TRACKS => {
                if csize > 16 * 1024 * 1024 {
                    return Ok(false);
                }
                let mut buf = vec![0u8; csize as usize];
                src.seek(SeekFrom::Start(payload_start))?;
                src.read_exact(&mut buf)?;
                tracks_payload = Some(buf);
            }
            ID_CLUSTER => {
                let peek_len = csize.min(48) as usize;
                let mut peek = vec![0u8; peek_len];
                src.seek(SeekFrom::Start(payload_start))?;
                src.read_exact(&mut peek)?;
                let mut ts = None;
                let mut track_num = 1u8;
                let mut p_off = 0usize;
                while p_off < peek.len() {
                    let Some((eid, eil)) = parse_id(&peek[p_off..]) else {
                        break;
                    };
                    let Some((esz, esl, _)) = parse_size(&peek[p_off + eil..]) else {
                        break;
                    };
                    let body_s = p_off + eil + esl;
                    if eid == ID_CLUSTER_TS {
                        let body_e = (body_s + esz as usize).min(peek.len());
                        ts = Some(read_uint(&peek[body_s..body_e]));
                    } else if eid == ID_SIMPLE_BLOCK || eid == ID_BLOCK_GROUP {
                        if let Some(&b0) = peek.get(body_s) {
                            track_num = b0 & 0x7F;
                        }
                        break;
                    }
                    p_off = body_s + esz as usize;
                }
                if let Some(ts_val) = ts {
                    clusters.push((ts_val, track_num, payload_start, csize));
                }
            }
            _ => {}
        }
        pos = payload_end;
    }

    let Some(tracks_payload) = tracks_payload else {
        return Ok(false);
    };
    if clusters.is_empty() {
        return Ok(false);
    }

    let ticks_per_sec = 1_000_000_000.0 / (timestamp_scale as f64);
    let start_ticks = (start_sec.max(0.0) * ticks_per_sec).round() as u64;
    let end_ticks = ((start_sec.max(0.0) + duration_sec.max(0.1)) * ticks_per_sec).round() as u64;

    // Anchor on the last track-1 (video keyframe) cluster at or before start_ticks
    let base_ts = clusters
        .iter()
        .filter(|(ts, trk, _, _)| *trk == 1 && *ts <= start_ticks + 50)
        .map(|(ts, _, _, _)| *ts)
        .next_back()
        .or_else(|| {
            clusters
                .iter()
                .filter(|(ts, _, _, _)| *ts <= start_ticks + 50)
                .map(|(ts, _, _, _)| *ts)
                .next_back()
        })
        .unwrap_or(clusters[0].0);

    let margin_ticks = (0.5 * ticks_per_sec).round() as u64;
    let audio_base_ts = clusters
        .iter()
        .filter(|(ts, trk, _, _)| *trk != 1 && *ts <= base_ts + 50)
        .map(|(ts, _, _, _)| *ts)
        .next_back()
        .unwrap_or(base_ts);

    let mut kept: Vec<(u64, u8, u64, u64)> = clusters
        .iter()
        .copied()
        .filter(|(ts, trk, _, _)| {
            let min_ts = if *trk == 1 { base_ts } else { audio_base_ts };
            *ts >= min_ts && *ts <= end_ticks + margin_ticks
        })
        .collect();
    if kept.is_empty() {
        kept.push(clusters[0]);
    }

    let last_ts = kept.last().map(|c| c.0).unwrap_or(base_ts);
    let clip_dur_ticks =
        ((last_ts.saturating_sub(base_ts)) as f64).max(duration_sec.max(0.1) * ticks_per_sec);

    let mut info = Vec::new();
    info.extend_from_slice(&uint_elem(ID_TIMESTAMP_SCALE, timestamp_scale));
    info.extend_from_slice(&elem(ID_DURATION, &clip_dur_ticks.to_be_bytes()));
    info.extend_from_slice(&elem(0x4D80, b"ruvic-pure-rust"));
    info.extend_from_slice(&elem(0x5741, b"ruvic-pure-rust"));

    out.write_all(&elem(ID_EBML, &ebml_header_payload))?;
    out.write_all(&id_bytes(ID_SEGMENT))?;
    out.write_all(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])?;
    out.write_all(&elem(ID_INFO, &info))?;
    out.write_all(&elem(ID_TRACKS, &tracks_payload))?;

    for (ts, _trk, c_start, c_size) in kept {
        if c_size > 32 * 1024 * 1024 {
            return Ok(false);
        }
        let mut buf = vec![0u8; c_size as usize];
        src.seek(SeekFrom::Start(c_start))?;
        src.read_exact(&mut buf)?;
        let shifted_ts = ts.saturating_sub(base_ts);
        let Some(rewritten) = rewrite_cluster_timestamp(&buf, shifted_ts) else {
            return Ok(false);
        };
        out.write_all(&rewritten)?;
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fmp4_track(handler: &[u8; 4], tfdt: u32, payload: &[u8]) -> Vec<u8> {
        let mut tkhd = vec![0u8; 84];
        tkhd[12..16].copy_from_slice(&7u32.to_be_bytes());
        let mut mdhd = vec![0u8; 24];
        mdhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
        let mut hdlr = vec![0u8; 24];
        hdlr[8..12].copy_from_slice(handler);
        let mut mdia = wrap_box(b"mdhd", &mdhd);
        mdia.extend_from_slice(&wrap_box(b"hdlr", &hdlr));
        let mut trak = wrap_box(b"tkhd", &tkhd);
        trak.extend_from_slice(&wrap_box(b"mdia", &mdia));
        let mut moov = build_mvhd(1000, 2000, 8);
        moov.extend_from_slice(&wrap_box(b"trak", &trak));
        moov.extend_from_slice(&wrap_box(b"mvex", &build_trex(7)));

        let mut mfhd = vec![0u8; 8];
        mfhd[4..8].copy_from_slice(&1u32.to_be_bytes());
        let mut tfhd = vec![0u8; 8];
        tfhd[4..8].copy_from_slice(&7u32.to_be_bytes());
        let mut tfdt_b = vec![0u8; 8];
        tfdt_b[4..8].copy_from_slice(&tfdt.to_be_bytes());
        let mut traf = wrap_box(b"tfhd", &tfhd);
        traf.extend_from_slice(&wrap_box(b"tfdt", &tfdt_b));
        let mut moof = wrap_box(b"mfhd", &mfhd);
        moof.extend_from_slice(&wrap_box(b"traf", &traf));

        let mut f = wrap_box(b"ftyp", b"iso5\0\0\0\0");
        f.extend_from_slice(&wrap_box(b"moov", &moov));
        f.extend_from_slice(&wrap_box(b"moof", &moof));
        f.extend_from_slice(&wrap_box(b"mdat", payload));
        f
    }

    #[test]
    fn fmp4_dual_mux_patches_track_ids_and_preserves_payloads() {
        let v = fmp4_track(b"vide", 0, b"VIDEOPAYLOAD");
        let a = fmp4_track(b"soun", 500, b"AUDIO");
        let mut out = Vec::new();
        assert!(try_mux_fmp4(&mut Cursor::new(v), &mut Cursor::new(a), &mut out).unwrap());

        let boxes = scan_top_level_boxes(&mut Cursor::new(out.clone()))
            .unwrap()
            .unwrap();
        let kinds: Vec<&[u8; 4]> = boxes.iter().map(|b| &b.kind).collect();
        assert_eq!(
            kinds,
            vec![b"ftyp", b"moov", b"moof", b"mdat", b"moof", b"mdat"]
        );
        let moov = &out[boxes[1].offset as usize + 8..(boxes[1].offset + boxes[1].size) as usize];
        assert_eq!(
            child_boxes(moov).iter().filter(|c| &c.0 == b"trak").count(),
            2
        );

        let moof1 = &out[boxes[2].offset as usize..(boxes[2].offset + boxes[2].size) as usize];
        let moof2 = &out[boxes[4].offset as usize..(boxes[4].offset + boxes[4].size) as usize];
        // mfhd seq at moof+8+12, tfhd track id at moof+8+16(mfhd)+8(traf hdr)+12
        assert_eq!(read_u32(moof1, 20), Some(1));
        assert_eq!(read_u32(moof2, 20), Some(2));
        assert_eq!(read_u32(moof1, 44), Some(1));
        assert_eq!(read_u32(moof2, 44), Some(2));
        assert!(out.windows(12).any(|w| w == b"VIDEOPAYLOAD"));
        assert!(out.windows(5).any(|w| w == b"AUDIO"));
    }

    fn webm_track(codec: &str, cluster_ts: &[u64]) -> Vec<u8> {
        let mut entry = uint_elem(ID_TRACK_NUMBER, 1);
        entry.extend_from_slice(&uint_elem(ID_TRACK_UID, 1));
        entry.extend_from_slice(&elem(0x86, codec.as_bytes()));
        entry.extend_from_slice(&elem(0x63A2, b"PRIV"));
        let mut seg = elem(ID_INFO, &uint_elem(ID_TIMESTAMP_SCALE, 1_000_000));
        seg.extend_from_slice(&elem(ID_TRACKS, &elem(ID_TRACK_ENTRY, &entry)));
        for &ts in cluster_ts {
            let mut c = uint_elem(ID_CLUSTER_TS, ts);
            c.extend_from_slice(&elem(ID_SIMPLE_BLOCK, &[0x81, 0, 0, 0x80, 0xAB, 0xCD]));
            seg.extend_from_slice(&elem(ID_CLUSTER, &c));
        }
        let mut f = elem(ID_EBML, &elem(0x4282, b"webm"));
        f.extend_from_slice(&elem(ID_SEGMENT, &seg));
        f
    }

    #[test]
    fn webm_dual_mux_interleaves_clusters_and_retracks_blocks() {
        let v = webm_track("V_VP9", &[0, 1000]);
        let a = webm_track("A_OPUS", &[0, 500, 1500]);
        let mut out = Vec::new();
        assert!(try_mux_webm(&mut Cursor::new(v), &mut Cursor::new(a), &mut out).unwrap());
        let parsed = load_webm_track(&mut Cursor::new(out.clone()));
        // Output has two tracks so single-track loader rejects it.
        assert!(parsed.unwrap().is_none());
        assert!(out.windows(5).any(|w| w == b"V_VP9"));
        assert!(out.windows(6).any(|w| w == b"A_OPUS"));
        assert!(out.windows(4).any(|w| w == b"PRIV"));
        let v_blocks = out.windows(4).filter(|w| *w == [0x81, 0, 0, 0x80]).count();
        let a_blocks = out.windows(4).filter(|w| *w == [0x82, 0, 0, 0x80]).count();
        assert_eq!((v_blocks, a_blocks), (2, 3));
    }
}
