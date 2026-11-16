#![allow(clippy::too_many_arguments)]
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use crate::domain::media_info::{ContainerInfo, MediaInfo};
use crate::domain::stream::{AudioCodec, ContainerFormat, StreamInfo, StreamKind, VideoCodec};
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StscEntry {
    pub first_chunk: u32,
    pub samples_per_chunk: u32,
    pub sample_desc_index: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TrackSampleTable {
    pub track_id: u32,
    pub handler_type: [u8; 4],
    pub sample_count: usize,
    pub uniform_sample_size: Option<u32>,
    pub sample_sizes: Vec<u32>,
    pub chunk_offsets: Vec<u64>,
    pub uses_co64: bool,
    pub samples_per_chunk: Vec<StscEntry>,
    pub sync_samples: Option<Vec<u32>>,
    pub time_to_sample: Vec<(u32, u32)>,
    pub colr: Option<Vec<u8>>,
    pub mdcv: Option<Vec<u8>>,
    pub clli: Option<Vec<u8>>,
    pub sv3d: Option<Vec<u8>>,
}

impl TrackSampleTable {
    /// Computes the absolute file offset and length for every sample in the track.
    pub fn compute_sample_ranges(&self) -> Vec<(u64, u32)> {
        let mut sample_ranges = Vec::with_capacity(self.sample_count);
        let num_chunks = self.chunk_offsets.len();
        if num_chunks == 0 {
            return sample_ranges;
        }

        let mut current_sample_idx = 0usize;
        for (chunk_idx, &chunk_offset) in self.chunk_offsets.iter().enumerate() {
            let chunk_num = (chunk_idx + 1) as u32; // 1-based chunk number
            let spc = self
                .samples_per_chunk
                .iter()
                .rfind(|entry| entry.first_chunk <= chunk_num)
                .map(|e| e.samples_per_chunk)
                .unwrap_or(1);

            let mut offset = chunk_offset;
            for _ in 0..spc {
                if current_sample_idx >= self.sample_count {
                    break;
                }
                let sz = if let Some(uniform) = self.uniform_sample_size {
                    uniform
                } else if current_sample_idx < self.sample_sizes.len() {
                    self.sample_sizes[current_sample_idx]
                } else {
                    0
                };
                sample_ranges.push((offset, sz));
                offset += sz as u64;
                current_sample_idx += 1;
            }
        }
        sample_ranges
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrackAuxBoxes {
    pub colr: Option<Vec<u8>>,
    pub mdcv: Option<Vec<u8>>,
    pub clli: Option<Vec<u8>>,
    pub sv3d: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ParsedMp4 {
    pub info: MediaInfo,
    pub mdat_offset: usize,
    pub mdat_len: usize,
    pub track_tables: Vec<TrackSampleTable>,
    pub moov_offset: usize,
    pub moov_len: usize,
    pub ftyp_len: usize,
}

pub fn parse_mp4_bytes(data: &[u8], path_label: &str) -> RuviResult<ParsedMp4> {
    if data.len() < 8 || &data[4..8] != b"ftyp" {
        return Err(RuviError::Integrity(
            "invalid ISO-BMFF/MP4 file: missing ftyp box".to_string(),
        ));
    }

    let mut pos = 0usize;
    let mut duration_seconds: Option<f64> = None;
    let mut streams: Vec<StreamInfo> = Vec::new();
    let mut track_tables: Vec<TrackSampleTable> = Vec::new();
    let mut mdat_offset = 0usize;
    let mut mdat_len = 0usize;
    let mut saw_moov = false;
    let mut moov_offset = 0usize;
    let mut moov_len = 0usize;
    let mut ftyp_len = 0usize;

    while pos + 8 <= data.len() {
        let (box_type, payload_start, box_end) = read_box_header(data, pos)?;
        let payload = &data[payload_start..box_end];
        match &box_type {
            b"ftyp" => {
                if ftyp_len == 0 {
                    ftyp_len = box_end - pos;
                }
            }
            b"moov" => {
                saw_moov = true;
                moov_offset = pos;
                moov_len = box_end - pos;
                parse_moov(
                    payload,
                    &mut duration_seconds,
                    &mut streams,
                    &mut track_tables,
                )?;
            }
            b"mdat" => {
                mdat_offset = payload_start;
                mdat_len = box_end - payload_start;
            }
            _ => {}
        }
        pos = box_end;
    }

    if pos != data.len() {
        return Err(RuviError::Integrity(
            "invalid MP4 container: trailing truncated box".to_string(),
        ));
    }

    if !saw_moov || streams.is_empty() {
        return Err(RuviError::Integrity(
            "invalid MP4 container: missing moov atom or media tracks".to_string(),
        ));
    }

    Ok(ParsedMp4 {
        info: MediaInfo {
            path: path_label.to_string(),
            container: ContainerInfo {
                format_name: "mov,mp4,m4a,3gp,3g2,mj2".to_string(),
                format_long_name: Some("QuickTime / ISO-BMFF MP4".to_string()),
                duration_seconds,
                size_bytes: Some(data.len() as u64),
                bit_rate: duration_seconds
                    .filter(|d| *d > 0.0)
                    .map(|d| ((data.len() as f64 * 8.0) / d) as u64),
            },
            streams,
            chapters: Vec::new(),
        },
        mdat_offset,
        mdat_len,
        track_tables,
        moov_offset,
        moov_len,
        ftyp_len,
    })
}

pub(crate) fn read_box_header(data: &[u8], pos: usize) -> RuviResult<([u8; 4], usize, usize)> {
    if pos + 8 > data.len() {
        return Err(RuviError::Integrity("truncated MP4 box header".to_string()));
    }
    let size32 =
        u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as u64;
    let box_type = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
    let (total_size, header_len) = if size32 == 1 {
        if pos + 16 > data.len() {
            return Err(RuviError::Integrity("truncated MP4 64-bit box".to_string()));
        }
        let s64 = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap());
        (s64, 16usize)
    } else if size32 == 0 {
        ((data.len() - pos) as u64, 8usize)
    } else {
        (size32, 8usize)
    };
    let remaining = (data.len() - pos) as u64;
    if total_size < header_len as u64 || total_size > remaining {
        return Err(RuviError::Integrity(format!(
            "invalid or truncated MP4 box '{}'",
            String::from_utf8_lossy(&box_type)
        )));
    }
    let end = pos + total_size as usize;
    Ok((box_type, pos + header_len, end))
}

pub(crate) fn parse_moov(
    moov: &[u8],
    duration_seconds: &mut Option<f64>,
    streams: &mut Vec<StreamInfo>,
    track_tables: &mut Vec<TrackSampleTable>,
) -> RuviResult<()> {
    let mut pos = 0usize;
    while pos + 8 <= moov.len() {
        let (box_type, pstart, end) = read_box_header(moov, pos)?;
        let payload = &moov[pstart..end];
        match &box_type {
            b"mvhd" => {
                *duration_seconds = parse_mvhd_duration(payload);
            }
            b"trak" => {
                if let Some((stream, table)) = parse_trak(payload, streams.len()) {
                    streams.push(stream);
                    track_tables.push(table);
                }
            }
            _ => {}
        }
        pos = end;
    }
    Ok(())
}

fn parse_mvhd_duration(payload: &[u8]) -> Option<f64> {
    if payload.len() < 20 {
        return None;
    }
    let version = payload[0];
    if version == 0 {
        let timescale = u32::from_be_bytes(payload[12..16].try_into().ok()?);
        let dur = u32::from_be_bytes(payload[16..20].try_into().ok()?);
        if timescale > 0 {
            Some(f64::from(dur) / f64::from(timescale))
        } else {
            None
        }
    } else if version == 1 && payload.len() >= 32 {
        let timescale = u32::from_be_bytes(payload[20..24].try_into().ok()?);
        let dur = u64::from_be_bytes(payload[24..32].try_into().ok()?);
        if timescale > 0 {
            Some(dur as f64 / f64::from(timescale))
        } else {
            None
        }
    } else {
        None
    }
}

fn parse_trak(trak: &[u8], index: usize) -> Option<(StreamInfo, TrackSampleTable)> {
    let mut tkhd_dims: (Option<u32>, Option<u32>) = (None, None);
    let mut track_id = (index + 1) as u32;
    let mut handler_type = [0u8; 4];
    let mut language: Option<String> = None;
    let mut stsd_codec: Option<String> = None;
    let mut stsd_dims: (Option<u32>, Option<u32>) = (None, None);
    let mut sample_rate: Option<u32> = None;
    let mut channels: Option<u16> = None;
    let mut sample_table = TrackSampleTable::default();
    let mut stream_info = StreamInfo::default();

    let mut pos = 0usize;
    while pos + 8 <= trak.len() {
        let Ok((btype, pstart, end)) = read_box_header(trak, pos) else {
            break;
        };
        let payload = &trak[pstart..end];
        if &btype == b"tkhd" {
            let (w, h, tid) = parse_tkhd(payload);
            tkhd_dims = (w, h);
            if let Some(id) = tid {
                track_id = id;
            }
        } else if &btype == b"mdia" {
            parse_mdia(
                payload,
                &mut handler_type,
                &mut language,
                &mut stsd_codec,
                &mut stsd_dims,
                &mut sample_rate,
                &mut channels,
                &mut sample_table,
                &mut stream_info,
            );
        }
        pos = end;
    }

    let kind = match &handler_type {
        b"vide" => StreamKind::Video,
        b"soun" => StreamKind::Audio,
        b"sbtl" | b"text" | b"subt" => StreamKind::Subtitle,
        _ => return None,
    };

    let codec_name = stsd_codec.unwrap_or_else(|| "unknown".to_string());
    let (width, height) = if kind == StreamKind::Video {
        (stsd_dims.0.or(tkhd_dims.0), stsd_dims.1.or(tkhd_dims.1))
    } else {
        (None, None)
    };

    stream_info.index = index;
    stream_info.kind = kind;
    stream_info.codec_name = codec_name;
    stream_info.width = width;
    stream_info.height = height;
    stream_info.fps = (kind == StreamKind::Video).then_some(30.0);
    stream_info.sample_rate = if kind == StreamKind::Audio {
        sample_rate
    } else {
        None
    };
    stream_info.channels = if kind == StreamKind::Audio {
        channels
    } else {
        None
    };
    stream_info.language = language;
    if kind == StreamKind::Video && stream_info.pixel_format.is_none() {
        stream_info.pixel_format = Some("yuv420p".to_string());
    }

    sample_table.track_id = track_id;
    sample_table.handler_type = handler_type;

    Some((stream_info, sample_table))
}

fn parse_tkhd(payload: &[u8]) -> (Option<u32>, Option<u32>, Option<u32>) {
    let version = payload.first().copied().unwrap_or(0);
    let min_len = if version == 1 { 96 } else { 84 };
    if payload.len() < min_len {
        return (None, None, None);
    }
    let track_id = if version == 1 && payload.len() >= 24 {
        Some(u32::from_be_bytes([
            payload[20],
            payload[21],
            payload[22],
            payload[23],
        ]))
    } else if payload.len() >= 16 {
        Some(u32::from_be_bytes([
            payload[12],
            payload[13],
            payload[14],
            payload[15],
        ]))
    } else {
        None
    };
    let w = u16::from_be_bytes([payload[min_len - 8], payload[min_len - 7]]) as u32;
    let h = u16::from_be_bytes([payload[min_len - 4], payload[min_len - 3]]) as u32;
    ((w > 0).then_some(w), (h > 0).then_some(h), track_id)
}

fn parse_mdia(
    mdia: &[u8],
    handler_type: &mut [u8; 4],
    language: &mut Option<String>,
    codec: &mut Option<String>,
    dims: &mut (Option<u32>, Option<u32>),
    sample_rate: &mut Option<u32>,
    channels: &mut Option<u16>,
    sample_table: &mut TrackSampleTable,
    stream: &mut StreamInfo,
) {
    let mut pos = 0usize;
    while pos + 8 <= mdia.len() {
        let Ok((btype, pstart, end)) = read_box_header(mdia, pos) else {
            break;
        };
        let payload = &mdia[pstart..end];
        match &btype {
            b"mdhd" => {
                *language = parse_mdhd_lang(payload);
            }
            b"hdlr" => {
                if payload.len() >= 12 {
                    handler_type.copy_from_slice(&payload[8..12]);
                }
            }
            b"minf" => {
                parse_minf(
                    payload,
                    codec,
                    dims,
                    sample_rate,
                    channels,
                    sample_table,
                    stream,
                );
            }
            _ => {}
        }
        pos = end;
    }
}

fn parse_mdhd_lang(payload: &[u8]) -> Option<String> {
    let lang_offset = if payload.first() == Some(&1) { 32 } else { 20 };
    if payload.len() < lang_offset + 2 {
        return None;
    }
    let code = u16::from_be_bytes([payload[lang_offset], payload[lang_offset + 1]]);
    if code == 0 {
        return None;
    }
    let f1 = ((code >> 10) & 0x1F) as u8;
    let f2 = ((code >> 5) & 0x1F) as u8;
    let f3 = (code & 0x1F) as u8;
    if !(1..=26).contains(&f1) || !(1..=26).contains(&f2) || !(1..=26).contains(&f3) {
        return None;
    }
    Some(format!(
        "{}{}{}",
        (f1 + 0x60) as char,
        (f2 + 0x60) as char,
        (f3 + 0x60) as char
    ))
}

fn parse_minf(
    minf: &[u8],
    codec: &mut Option<String>,
    dims: &mut (Option<u32>, Option<u32>),
    sample_rate: &mut Option<u32>,
    channels: &mut Option<u16>,
    sample_table: &mut TrackSampleTable,
    stream: &mut StreamInfo,
) {
    let mut pos = 0usize;
    while pos + 8 <= minf.len() {
        let Ok((btype, pstart, end)) = read_box_header(minf, pos) else {
            break;
        };
        let payload = &minf[pstart..end];
        if &btype == b"stbl" {
            let mut spos = 0usize;
            while spos + 8 <= payload.len() {
                let Ok((stype, spstart, send)) = read_box_header(payload, spos) else {
                    break;
                };
                let sbody = &payload[spstart..send];
                match &stype {
                    b"stsd" => parse_stsd(
                        sbody,
                        codec,
                        dims,
                        sample_rate,
                        channels,
                        sample_table,
                        stream,
                    ),
                    b"stts" => parse_stts(sbody, sample_table),
                    b"stss" => parse_stss(sbody, sample_table),
                    b"stsc" => parse_stsc(sbody, sample_table),
                    b"stsz" => parse_stsz(sbody, sample_table),
                    b"stco" => parse_stco(sbody, sample_table),
                    b"co64" => parse_co64(sbody, sample_table),
                    _ => {}
                }
                spos = send;
            }
        }
        pos = end;
    }
}

fn parse_stts(payload: &[u8], table: &mut TrackSampleTable) {
    if payload.len() < 8 {
        return;
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap_or([0; 4])) as usize;
    let mut pos = 8usize;
    let mut total_samples = 0usize;
    for _ in 0..count {
        if pos + 8 > payload.len() {
            break;
        }
        let sample_cnt = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap());
        let sample_delta = u32::from_be_bytes(payload[pos + 4..pos + 8].try_into().unwrap());
        table.time_to_sample.push((sample_cnt, sample_delta));
        total_samples = total_samples.saturating_add(sample_cnt as usize);
        pos += 8;
    }
    if table.sample_count == 0 {
        table.sample_count = total_samples;
    }
}

fn parse_stss(payload: &[u8], table: &mut TrackSampleTable) {
    if payload.len() < 8 {
        return;
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap_or([0; 4])) as usize;
    let mut pos = 8usize;
    let mut syncs = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 4 > payload.len() {
            break;
        }
        let num = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap());
        syncs.push(num);
        pos += 4;
    }
    table.sync_samples = Some(syncs);
}

fn parse_stsc(payload: &[u8], table: &mut TrackSampleTable) {
    if payload.len() < 8 {
        return;
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap_or([0; 4])) as usize;
    let mut pos = 8usize;
    for _ in 0..count {
        if pos + 12 > payload.len() {
            break;
        }
        let first_chunk = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap());
        let spc = u32::from_be_bytes(payload[pos + 4..pos + 8].try_into().unwrap());
        let desc_idx = u32::from_be_bytes(payload[pos + 8..pos + 12].try_into().unwrap());
        table.samples_per_chunk.push(StscEntry {
            first_chunk,
            samples_per_chunk: spc,
            sample_desc_index: desc_idx,
        });
        pos += 12;
    }
}

fn parse_stsz(payload: &[u8], table: &mut TrackSampleTable) {
    if payload.len() < 12 {
        return;
    }
    let uniform = u32::from_be_bytes(payload[4..8].try_into().unwrap_or([0; 4]));
    let count = u32::from_be_bytes(payload[8..12].try_into().unwrap_or([0; 4])) as usize;
    table.sample_count = count;
    if uniform > 0 {
        table.uniform_sample_size = Some(uniform);
    } else {
        let mut pos = 12usize;
        table.sample_sizes.reserve(count);
        for _ in 0..count {
            if pos + 4 > payload.len() {
                break;
            }
            let sz = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap());
            table.sample_sizes.push(sz);
            pos += 4;
        }
    }
}

fn parse_stco(payload: &[u8], table: &mut TrackSampleTable) {
    if payload.len() < 8 {
        return;
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap_or([0; 4])) as usize;
    let mut pos = 8usize;
    table.chunk_offsets.reserve(count);
    for _ in 0..count {
        if pos + 4 > payload.len() {
            break;
        }
        let off = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap());
        table.chunk_offsets.push(off as u64);
        pos += 4;
    }
    table.uses_co64 = false;
}

fn parse_co64(payload: &[u8], table: &mut TrackSampleTable) {
    if payload.len() < 8 {
        return;
    }
    let count = u32::from_be_bytes(payload[4..8].try_into().unwrap_or([0; 4])) as usize;
    let mut pos = 8usize;
    table.chunk_offsets.reserve(count);
    for _ in 0..count {
        if pos + 8 > payload.len() {
            break;
        }
        let off = u64::from_be_bytes(payload[pos..pos + 8].try_into().unwrap());
        table.chunk_offsets.push(off);
        pos += 8;
    }
    table.uses_co64 = true;
}

fn parse_stsd(
    stsd: &[u8],
    codec: &mut Option<String>,
    dims: &mut (Option<u32>, Option<u32>),
    sample_rate: &mut Option<u32>,
    channels: &mut Option<u16>,
    sample_table: &mut TrackSampleTable,
    stream: &mut StreamInfo,
) {
    // version(1) + flags(3) + entry_count(4) + first entry header(8)
    if stsd.len() < 16 {
        return;
    }
    let entry_fourcc = &stsd[12..16];
    let entry_body = &stsd[16..];
    *codec = Some(map_mp4_fourcc(entry_fourcc));

    // VisualSampleEntry: 8 bytes base + 16 reserved + width(2) + height(2) => bytes 24..28 in entry_body
    if entry_body.len() >= 28
        && matches!(
            entry_fourcc,
            b"avc1" | b"avc3" | b"hev1" | b"hvc1" | b"vp09" | b"av01" | b"mp4v"
        )
    {
        let w = u16::from_be_bytes([entry_body[24], entry_body[25]]) as u32;
        let h = u16::from_be_bytes([entry_body[26], entry_body[27]]) as u32;
        *dims = ((w > 0).then_some(w), (h > 0).then_some(h));

        // Parse auxiliary boxes in VisualSampleEntry (starts after standard 78-byte header)
        if entry_body.len() > 78 {
            let mut sub_pos = 78usize;
            while sub_pos + 8 <= entry_body.len() {
                let Ok((sub_type, sub_start, sub_end)) = read_box_header(entry_body, sub_pos)
                else {
                    break;
                };
                let sub_payload = &entry_body[sub_start..sub_end];
                let full_box = &entry_body[sub_pos..sub_end];
                match &sub_type {
                    b"colr" => {
                        sample_table.colr = Some(full_box.to_vec());
                        if sub_payload.len() >= 11 && &sub_payload[0..4] == b"nclx" {
                            let cp = u16::from_be_bytes([sub_payload[4], sub_payload[5]]);
                            let ct = u16::from_be_bytes([sub_payload[6], sub_payload[7]]);
                            let cm = u16::from_be_bytes([sub_payload[8], sub_payload[9]]);
                            let fr = (sub_payload[10] & 0x80) != 0;
                            stream.color_primaries = Some(cp);
                            stream.color_transfer = Some(ct);
                            stream.color_matrix = Some(cm);
                            stream.color_full_range = Some(fr);
                        }
                    }
                    b"mdcv" => {
                        sample_table.mdcv = Some(full_box.to_vec());
                        stream.mastering_display = Some(sub_payload.to_vec());
                    }
                    b"clli" => {
                        sample_table.clli = Some(full_box.to_vec());
                        stream.content_light_level = Some(sub_payload.to_vec());
                    }
                    b"sv3d" | b"proj" => {
                        sample_table.sv3d = Some(full_box.to_vec());
                        stream.spatial_360 = Some(full_box.to_vec());
                    }
                    _ => {}
                }
                sub_pos = sub_end;
            }
        }
    }

    // AudioSampleEntry: 8 bytes base + 8 reserved + channels(2 at 16..18) + samplesize(2) + 4 reserved + sampleRate(4 at 24..28)
    if entry_body.len() >= 28
        && matches!(
            entry_fourcc,
            b"mp4a" | b"Opus" | b"opus" | b"fLaC" | b".mp3" | b"mp3 " | b"lpcm"
        )
    {
        let ch = u16::from_be_bytes([entry_body[16], entry_body[17]]);
        let sr = u16::from_be_bytes([entry_body[24], entry_body[25]]) as u32;
        if ch > 0 {
            *channels = Some(ch);
        }
        if sr > 0 {
            *sample_rate = Some(sr);
        }
    }
}

fn map_mp4_fourcc(fourcc: &[u8]) -> String {
    match fourcc {
        b"avc1" | b"avc3" => "h264".to_string(),
        b"hev1" | b"hvc1" => "hevc".to_string(),
        b"vp09" => "vp9".to_string(),
        b"av01" => "av1".to_string(),
        b"mp4a" => "aac".to_string(),
        b"Opus" | b"opus" => "opus".to_string(),
        b"fLaC" => "flac".to_string(),
        b".mp3" | b"mp3 " => "mp3".to_string(),
        b"lpcm" | b"sowt" | b"twos" => "pcm_s16le".to_string(),
        other => String::from_utf8_lossy(other).trim().to_ascii_lowercase(),
    }
}

pub struct Mp4BuildSpec<'a> {
    pub container: ContainerFormat,
    pub duration_seconds: f64,
    pub video: Option<(VideoCodec, u32, u32)>,
    pub audio: Option<(AudioCodec, u32, u16)>,
    pub mdat_payload: &'a [u8],
    pub video_aux: Option<TrackAuxBoxes>,
}

/// Builds a valid faststart ISO-BMFF MP4/M4A/MOV binary (`ftyp` + `moov` + `mdat`) in pure Rust.
pub fn build_mp4_bytes(spec: &Mp4BuildSpec<'_>) -> Vec<u8> {
    let major_brand = match spec.container {
        ContainerFormat::M4a => b"M4A ",
        ContainerFormat::Mov => b"qt  ",
        _ => b"isom",
    };

    let mut ftyp_body = Vec::new();
    ftyp_body.extend_from_slice(major_brand);
    ftyp_body.extend_from_slice(&512u32.to_be_bytes());
    ftyp_body.extend_from_slice(b"isomiso2avc1mp41");
    let ftyp = wrap_box(b"ftyp", &ftyp_body);

    let timescale = 1000u32;
    let duration_units = (spec.duration_seconds.max(0.1) * f64::from(timescale)).round() as u32;

    let payload = if spec.mdat_payload.is_empty() {
        &b"RUVIC_MEDIA_DATA"[..]
    } else {
        spec.mdat_payload
    };

    // Calculate moov box layout with true chunk offset
    let dummy_moov =
        build_moov_container(timescale, duration_units, spec, payload.len() as u32, 0, 0);
    let mdat_hdr_len = 8u64;
    let first_chunk_offset = ftyp.len() as u64 + dummy_moov.len() as u64 + mdat_hdr_len;

    let video_offset = first_chunk_offset;
    let audio_offset = if spec.video.is_some() && spec.audio.is_some() {
        first_chunk_offset + (payload.len() as u64 / 2)
    } else {
        first_chunk_offset
    };

    let moov = build_moov_container(
        timescale,
        duration_units,
        spec,
        payload.len() as u32,
        video_offset,
        audio_offset,
    );

    let mdat = wrap_box(b"mdat", payload);

    let mut out = Vec::with_capacity(ftyp.len() + moov.len() + mdat.len());
    out.extend_from_slice(&ftyp);
    out.extend_from_slice(&moov);
    out.extend_from_slice(&mdat);
    out
}

fn build_moov_container(
    timescale: u32,
    duration_units: u32,
    spec: &Mp4BuildSpec<'_>,
    payload_len: u32,
    video_chunk_offset: u64,
    audio_chunk_offset: u64,
) -> Vec<u8> {
    let mut traks = Vec::new();
    let mut next_track_id = 1u32;

    let (video_sample_size, audio_sample_size) = if spec.video.is_some() && spec.audio.is_some() {
        let v_len = payload_len / 2;
        let a_len = payload_len.saturating_sub(v_len);
        (v_len.max(1), a_len.max(1))
    } else {
        (payload_len.max(16), payload_len.max(16))
    };

    if let Some((vcodec, w, h)) = spec.video {
        let fourcc = match vcodec {
            VideoCodec::H265 => *b"hvc1",
            VideoCodec::Vp9 => *b"vp09",
            VideoCodec::Av1 => *b"av01",
            VideoCodec::H264 | VideoCodec::Copy => *b"avc1",
        };
        traks.push(build_video_trak(
            next_track_id,
            timescale,
            duration_units,
            w.max(2),
            h.max(2),
            fourcc,
            video_sample_size,
            video_chunk_offset,
            spec.video_aux.as_ref(),
        ));
        next_track_id += 1;
    }

    if let Some((acodec, sr, ch)) = spec.audio {
        let fourcc = match acodec {
            AudioCodec::Mp3 => *b".mp3",
            AudioCodec::Opus => *b"Opus",
            AudioCodec::Flac => *b"fLaC",
            AudioCodec::PcmS16Le => *b"lpcm",
            _ => *b"mp4a",
        };
        traks.push(build_audio_trak(
            next_track_id,
            timescale,
            duration_units,
            sr.max(8000),
            ch.max(1),
            fourcc,
            audio_sample_size,
            audio_chunk_offset,
        ));
        next_track_id += 1;
    }

    let mvhd = build_mvhd(timescale, duration_units, next_track_id);
    let mut moov_body = mvhd;
    for t in traks {
        moov_body.extend_from_slice(&t);
    }
    wrap_box(b"moov", &moov_body)
}

pub(crate) fn wrap_box(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let total = (8 + body.len()) as u32;
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    out
}

fn build_mvhd(timescale: u32, duration: u32, next_track_id: u32) -> Vec<u8> {
    let mut b = vec![0u8; 100];
    b[12..16].copy_from_slice(&timescale.to_be_bytes());
    b[16..20].copy_from_slice(&duration.to_be_bytes());
    b[20..24].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[24..26].copy_from_slice(&0x0100u16.to_be_bytes());
    b[36..40].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[52..56].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[68..72].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    b[96..100].copy_from_slice(&next_track_id.to_be_bytes());
    wrap_box(b"mvhd", &b)
}

fn build_tkhd(track_id: u32, duration: u32, width: u32, height: u32, is_audio: bool) -> Vec<u8> {
    let mut b = vec![0u8; 84];
    b[3] = 0x07;
    b[12..16].copy_from_slice(&track_id.to_be_bytes());
    b[20..24].copy_from_slice(&duration.to_be_bytes());
    if is_audio {
        b[36..38].copy_from_slice(&0x0100u16.to_be_bytes());
    }
    b[40..44].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[56..60].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[72..76].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    b[76..80].copy_from_slice(&(width << 16).to_be_bytes());
    b[80..84].copy_from_slice(&(height << 16).to_be_bytes());
    wrap_box(b"tkhd", &b)
}

fn build_mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut b = vec![0u8; 24];
    b[12..16].copy_from_slice(&timescale.to_be_bytes());
    b[16..20].copy_from_slice(&duration.to_be_bytes());
    b[20..22].copy_from_slice(&0x55C4u16.to_be_bytes());
    wrap_box(b"mdhd", &b)
}

fn build_hdlr(handler_type: &[u8; 4], name: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 24];
    b[8..12].copy_from_slice(handler_type);
    b.extend_from_slice(name);
    b.push(0);
    wrap_box(b"hdlr", &b)
}

fn build_video_trak(
    track_id: u32,
    timescale: u32,
    duration: u32,
    width: u32,
    height: u32,
    fourcc: [u8; 4],
    sample_size: u32,
    chunk_offset: u64,
    aux: Option<&TrackAuxBoxes>,
) -> Vec<u8> {
    let tkhd = build_tkhd(track_id, duration, width, height, false);
    let mdhd = build_mdhd(timescale, duration);
    let hdlr = build_hdlr(b"vide", b"VideoHandler");

    let mut entry = vec![0u8; 78];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[24..26].copy_from_slice(&(width as u16).to_be_bytes());
    entry[26..28].copy_from_slice(&(height as u16).to_be_bytes());
    entry[28..32].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    entry[32..36].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    entry[40..42].copy_from_slice(&1u16.to_be_bytes());
    entry[74..76].copy_from_slice(&24u16.to_be_bytes());
    entry[76..78].copy_from_slice(&0xFFFFu16.to_be_bytes());

    if let Some(aux_boxes) = aux {
        if let Some(ref c) = aux_boxes.colr {
            entry.extend_from_slice(c);
        }
        if let Some(ref m) = aux_boxes.mdcv {
            entry.extend_from_slice(m);
        }
        if let Some(ref cl) = aux_boxes.clli {
            entry.extend_from_slice(cl);
        }
        if let Some(ref s) = aux_boxes.sv3d {
            entry.extend_from_slice(s);
        }
    }

    let sample_entry = wrap_box(&fourcc, &entry);

    let mut stsd_body = vec![0u8; 8];
    stsd_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_body.extend_from_slice(&sample_entry);
    let stsd = wrap_box(b"stsd", &stsd_body);

    let stbl = build_minimal_stbl(stsd, duration, sample_size, chunk_offset);
    let vmhd = wrap_box(b"vmhd", &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    let dinf = build_dinf();

    let mut minf_body = vmhd;
    minf_body.extend_from_slice(&dinf);
    minf_body.extend_from_slice(&stbl);
    let minf = wrap_box(b"minf", &minf_body);

    let mut mdia_body = mdhd;
    mdia_body.extend_from_slice(&hdlr);
    mdia_body.extend_from_slice(&minf);
    let mdia = wrap_box(b"mdia", &mdia_body);

    let mut trak_body = tkhd;
    trak_body.extend_from_slice(&mdia);
    wrap_box(b"trak", &trak_body)
}

fn build_audio_trak(
    track_id: u32,
    timescale: u32,
    duration: u32,
    sample_rate: u32,
    channels: u16,
    fourcc: [u8; 4],
    sample_size: u32,
    chunk_offset: u64,
) -> Vec<u8> {
    let tkhd = build_tkhd(track_id, duration, 0, 0, true);
    let mdhd = build_mdhd(timescale, duration);
    let hdlr = build_hdlr(b"soun", b"SoundHandler");

    let mut entry = vec![0u8; 28];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[16..18].copy_from_slice(&channels.to_be_bytes());
    entry[18..20].copy_from_slice(&16u16.to_be_bytes());
    entry[24..28].copy_from_slice(&(sample_rate << 16).to_be_bytes());
    let sample_entry = wrap_box(&fourcc, &entry);

    let mut stsd_body = vec![0u8; 8];
    stsd_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_body.extend_from_slice(&sample_entry);
    let stsd = wrap_box(b"stsd", &stsd_body);

    let stbl = build_minimal_stbl(stsd, duration, sample_size, chunk_offset);
    let smhd = wrap_box(b"smhd", &[0u8; 8]);
    let dinf = build_dinf();

    let mut minf_body = smhd;
    minf_body.extend_from_slice(&dinf);
    minf_body.extend_from_slice(&stbl);
    let minf = wrap_box(b"minf", &minf_body);

    let mut mdia_body = mdhd;
    mdia_body.extend_from_slice(&hdlr);
    mdia_body.extend_from_slice(&minf);
    let mdia = wrap_box(b"mdia", &mdia_body);

    let mut trak_body = tkhd;
    trak_body.extend_from_slice(&mdia);
    wrap_box(b"trak", &trak_body)
}

fn build_dinf() -> Vec<u8> {
    let url_box = wrap_box(b"url ", &[0, 0, 0, 1]);
    let mut dref_body = vec![0u8; 8];
    dref_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    dref_body.extend_from_slice(&url_box);
    let dref = wrap_box(b"dref", &dref_body);
    wrap_box(b"dinf", &dref)
}

fn build_minimal_stbl(
    stsd: Vec<u8>,
    duration: u32,
    sample_size: u32,
    chunk_offset: u64,
) -> Vec<u8> {
    let mut stts = vec![0u8; 16];
    stts[4..8].copy_from_slice(&1u32.to_be_bytes());
    stts[8..12].copy_from_slice(&1u32.to_be_bytes());
    stts[12..16].copy_from_slice(&duration.to_be_bytes());

    let mut stsc = vec![0u8; 20];
    stsc[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsc[8..12].copy_from_slice(&1u32.to_be_bytes());
    stsc[12..16].copy_from_slice(&1u32.to_be_bytes());
    stsc[16..20].copy_from_slice(&1u32.to_be_bytes());

    let mut stsz = vec![0u8; 12];
    stsz[4..8].copy_from_slice(&sample_size.to_be_bytes());
    stsz[8..12].copy_from_slice(&1u32.to_be_bytes());

    let mut body = stsd;
    body.extend_from_slice(&wrap_box(b"stts", &stts));
    body.extend_from_slice(&wrap_box(b"stsc", &stsc));
    body.extend_from_slice(&wrap_box(b"stsz", &stsz));

    if chunk_offset > u32::MAX as u64 {
        let mut co64 = vec![0u8; 16];
        co64[4..8].copy_from_slice(&1u32.to_be_bytes());
        co64[8..16].copy_from_slice(&chunk_offset.to_be_bytes());
        body.extend_from_slice(&wrap_box(b"co64", &co64));
    } else {
        let mut stco = vec![0u8; 12];
        stco[4..8].copy_from_slice(&1u32.to_be_bytes());
        stco[8..12].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
        body.extend_from_slice(&wrap_box(b"stco", &stco));
    }

    let _ = Cursor::new(&body);
    wrap_box(b"stbl", &body)
}

/// Shifts chunk offsets in-place inside `moov` bytes when moving `moov` before `mdat`.
pub fn shift_moov_chunk_offsets(moov_bytes: &[u8], shift: i64) -> RuviResult<Vec<u8>> {
    let mut updated = moov_bytes.to_vec();
    let ok = shift_container_in_place(&mut updated, 0, moov_bytes.len(), shift)?;
    if !ok {
        // Upgrade from stco to co64 is needed (occurs if chunk offsets exceed 4GB)
        return upgrade_moov_to_co64(moov_bytes, shift);
    }
    Ok(updated)
}

fn shift_container_in_place(
    data: &mut [u8],
    start: usize,
    end: usize,
    shift: i64,
) -> RuviResult<bool> {
    let mut pos = start;
    while pos + 8 <= end {
        let size32 =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as u64;
        let btype = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let (total_size, hlen) = if size32 == 1 {
            if pos + 16 > end {
                return Err(RuviError::Integrity("truncated box".to_string()));
            }
            let s64 = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap());
            (s64, 16usize)
        } else if size32 == 0 {
            ((end - pos) as u64, 8usize)
        } else {
            (size32, 8usize)
        };
        let box_end = pos + total_size as usize;
        if box_end > end {
            break;
        }

        match &btype {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => {
                let ok = shift_container_in_place(data, pos + hlen, box_end, shift)?;
                if !ok {
                    return Ok(false);
                }
            }
            b"stco" => {
                let pstart = pos + hlen;
                if pstart + 8 <= box_end {
                    let count = u32::from_be_bytes(data[pstart + 4..pstart + 8].try_into().unwrap())
                        as usize;
                    let mut entry_pos = pstart + 8;
                    for _ in 0..count {
                        if entry_pos + 4 > box_end {
                            break;
                        }
                        let old_off =
                            u32::from_be_bytes(data[entry_pos..entry_pos + 4].try_into().unwrap())
                                as u64;
                        let new_off = (old_off as i64 + shift) as u64;
                        if new_off > u32::MAX as u64 {
                            return Ok(false);
                        }
                        data[entry_pos..entry_pos + 4]
                            .copy_from_slice(&(new_off as u32).to_be_bytes());
                        entry_pos += 4;
                    }
                }
            }
            b"co64" => {
                let pstart = pos + hlen;
                if pstart + 8 <= box_end {
                    let count = u32::from_be_bytes(data[pstart + 4..pstart + 8].try_into().unwrap())
                        as usize;
                    let mut entry_pos = pstart + 8;
                    for _ in 0..count {
                        if entry_pos + 8 > box_end {
                            break;
                        }
                        let old_off =
                            u64::from_be_bytes(data[entry_pos..entry_pos + 8].try_into().unwrap());
                        let new_off = (old_off as i64 + shift) as u64;
                        data[entry_pos..entry_pos + 8].copy_from_slice(&new_off.to_be_bytes());
                        entry_pos += 8;
                    }
                }
            }
            _ => {}
        }
        pos = box_end;
    }
    Ok(true)
}

fn upgrade_moov_to_co64(moov_bytes: &[u8], shift: i64) -> RuviResult<Vec<u8>> {
    let mut duration_seconds = None;
    let mut streams = Vec::new();
    let mut track_tables = Vec::new();
    let pstart = if moov_bytes.len() >= 8 && &moov_bytes[4..8] == b"moov" {
        8
    } else {
        0
    };
    parse_moov(
        &moov_bytes[pstart..],
        &mut duration_seconds,
        &mut streams,
        &mut track_tables,
    )?;

    let timescale = 1000u32;
    let dur_units =
        (duration_seconds.unwrap_or(1.0).max(0.1) * f64::from(timescale)).round() as u32;

    let mut traks = Vec::new();
    let mut next_track_id = 1u32;

    for (s, t) in streams.iter().zip(track_tables.iter()) {
        let chunk_off = t.chunk_offsets.first().copied().unwrap_or(0);
        let new_chunk_off = (chunk_off as i64 + shift) as u64;
        let sample_sz = t
            .uniform_sample_size
            .unwrap_or_else(|| t.sample_sizes.first().copied().unwrap_or(16));

        if s.kind == StreamKind::Video {
            let vc = VideoCodec::from_name(&s.codec_name).unwrap_or(VideoCodec::H264);
            let fourcc = match vc {
                VideoCodec::H265 => *b"hvc1",
                VideoCodec::Vp9 => *b"vp09",
                VideoCodec::Av1 => *b"av01",
                _ => *b"avc1",
            };
            let aux = TrackAuxBoxes {
                colr: t.colr.clone(),
                mdcv: t.mdcv.clone(),
                clli: t.clli.clone(),
                sv3d: t.sv3d.clone(),
            };
            traks.push(build_video_trak(
                next_track_id,
                timescale,
                dur_units,
                s.width.unwrap_or(1280),
                s.height.unwrap_or(720),
                fourcc,
                sample_sz,
                new_chunk_off,
                Some(&aux),
            ));
            next_track_id += 1;
        } else if s.kind == StreamKind::Audio {
            let ac = AudioCodec::from_name(&s.codec_name).unwrap_or(AudioCodec::Aac);
            let fourcc = match ac {
                AudioCodec::Mp3 => *b".mp3",
                AudioCodec::Opus => *b"Opus",
                AudioCodec::Flac => *b"fLaC",
                AudioCodec::PcmS16Le => *b"lpcm",
                _ => *b"mp4a",
            };
            traks.push(build_audio_trak(
                next_track_id,
                timescale,
                dur_units,
                s.sample_rate.unwrap_or(48000),
                s.channels.unwrap_or(2),
                fourcc,
                sample_sz,
                new_chunk_off,
            ));
            next_track_id += 1;
        }
    }

    let mvhd = build_mvhd(timescale, dur_units, next_track_id);
    let mut moov_body = mvhd;
    for t in traks {
        moov_body.extend_from_slice(&t);
    }
    Ok(wrap_box(b"moov", &moov_body))
}

/// Reorganizes an existing MP4 file so that `moov` is placed before `mdat` (faststart),
/// recalculating all `stco`/`co64` chunk offsets with O(1) memory bounds.
pub fn rewrite_mp4_faststart<R: Read + Seek, W: Write>(
    reader: &mut R,
    writer: &mut W,
    file_len: u64,
) -> RuviResult<()> {
    reader.seek(SeekFrom::Start(0))?;
    let mut ftyp_bytes = Vec::new();
    let mut moov_bytes = Vec::new();
    let mut mdat_hdr_len = 0u64;
    let mut mdat_payload_offset = 0u64;
    let mut mdat_payload_len = 0u64;
    let mut moov_offset = 0u64;
    let mut pos = 0u64;

    while pos + 8 <= file_len {
        reader.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 16];
        let n = reader.read(&mut hdr)?;
        if n < 8 {
            break;
        }
        let size32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let btype = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let (total_size, hlen) = if size32 == 1 {
            if n < 16 {
                return Err(RuviError::Integrity("truncated 64-bit MP4 box".to_string()));
            }
            let s64 = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
            (s64, 16u64)
        } else if size32 == 0 {
            (file_len - pos, 8u64)
        } else {
            (size32, 8u64)
        };

        if total_size < hlen || total_size > file_len - pos {
            return Err(RuviError::Integrity("corrupt MP4 box size".to_string()));
        }

        match &btype {
            b"ftyp" => {
                ftyp_bytes.resize(total_size as usize, 0);
                reader.seek(SeekFrom::Start(pos))?;
                reader.read_exact(&mut ftyp_bytes)?;
            }
            b"moov" => {
                moov_offset = pos;
                moov_bytes.resize(total_size as usize, 0);
                reader.seek(SeekFrom::Start(pos))?;
                reader.read_exact(&mut moov_bytes)?;
            }
            b"mdat" => {
                mdat_hdr_len = hlen;
                mdat_payload_offset = pos + hlen;
                mdat_payload_len = total_size - hlen;
            }
            _ => {}
        }
        pos += total_size;
    }

    if moov_bytes.is_empty() || mdat_payload_len == 0 {
        return Err(RuviError::Integrity(
            "cannot faststart MP4: missing moov or mdat box".to_string(),
        ));
    }

    // If moov is already before mdat, stream directly!
    if moov_offset < mdat_payload_offset {
        reader.seek(SeekFrom::Start(0))?;
        copy_chunked(reader, writer, file_len, 64 * 1024)?;
        return Ok(());
    }

    let ftyp_len = ftyp_bytes.len() as u64;
    let orig_mdat_offset = mdat_payload_offset;
    let shift =
        (ftyp_len + moov_bytes.len() as u64 + mdat_hdr_len) as i64 - orig_mdat_offset as i64;

    let updated_moov = shift_moov_chunk_offsets(&moov_bytes, shift)?;

    writer.write_all(&ftyp_bytes)?;
    writer.write_all(&updated_moov)?;

    // Write mdat header
    if mdat_hdr_len == 16 {
        let total_size = mdat_payload_len + 16;
        let mut hdr = [0u8; 16];
        hdr[0..4].copy_from_slice(&1u32.to_be_bytes());
        hdr[4..8].copy_from_slice(b"mdat");
        hdr[8..16].copy_from_slice(&total_size.to_be_bytes());
        writer.write_all(&hdr)?;
    } else {
        let total_size = (mdat_payload_len + 8) as u32;
        let mut hdr = [0u8; 8];
        hdr[0..4].copy_from_slice(&total_size.to_be_bytes());
        hdr[4..8].copy_from_slice(b"mdat");
        writer.write_all(&hdr)?;
    }

    // Stream mdat payload from input
    reader.seek(SeekFrom::Start(orig_mdat_offset))?;
    copy_chunked(reader, writer, mdat_payload_len, 64 * 1024)?;

    Ok(())
}

/// Extracts real AAC access units via sample tables into valid ADTS frames.
pub fn extract_aac_adts_stream<R: Read + Seek, W: Write>(
    reader: &mut R,
    writer: &mut W,
    audio_table: &TrackSampleTable,
    sample_rate: u32,
    channels: u16,
) -> RuviResult<usize> {
    let file_len = reader.seek(SeekFrom::End(0))?;
    let sample_ranges = audio_table.compute_sample_ranges();
    let sr_idx = match sample_rate {
        96000 => 0,
        88200 => 1,
        64000 => 2,
        48000 => 3,
        44100 => 4,
        32000 => 5,
        24000 => 6,
        22050 => 7,
        16000 => 8,
        12000 => 9,
        11025 => 10,
        8000 => 11,
        7350 => 12,
        _ => 4,
    };
    let ch_u8 = channels.clamp(1, 7) as u8;

    let mut sample_buf = vec![0u8; 8192];
    let mut frames_written = 0usize;

    for (offset, sz) in sample_ranges {
        if sz == 0 || offset >= file_len {
            continue;
        }
        let to_read = (sz as u64).min(file_len - offset) as usize;
        if to_read == 0 {
            continue;
        }
        if sample_buf.len() < to_read {
            sample_buf.resize(to_read, 0);
        }
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut sample_buf[..to_read])?;

        let frame_len = (7 + to_read) as u16;
        let mut hdr = [0u8; 7];
        hdr[0] = 0xFF;
        hdr[1] = 0xF1;
        hdr[2] = (1 << 6) | (sr_idx << 2) | (ch_u8 >> 2);
        hdr[3] = ((ch_u8 & 0x03) << 6) | ((frame_len >> 11) as u8 & 0x03);
        hdr[4] = ((frame_len >> 3) & 0xFF) as u8;
        hdr[5] = (((frame_len & 0x07) as u8) << 5) | 0x1F;
        hdr[6] = 0xFC;

        writer.write_all(&hdr)?;
        writer.write_all(&sample_buf[..to_read])?;
        frames_written += 1;
    }

    Ok(frames_written)
}

/// Multiplexes separate video and audio input streams into a single synchronized MP4 container.
pub fn multiplex_mp4_dual_input<R1: Read + Seek, R2: Read + Seek, W: Write>(
    video_reader: &mut R1,
    audio_reader: &mut R2,
    writer: &mut W,
    video_info: &StreamInfo,
    audio_info: &StreamInfo,
    video_payload_offset: u64,
    video_payload_len: u64,
    audio_payload_offset: u64,
    audio_payload_len: u64,
    duration_seconds: f64,
) -> RuviResult<()> {
    let mut ftyp_body = Vec::new();
    ftyp_body.extend_from_slice(b"isom");
    ftyp_body.extend_from_slice(&512u32.to_be_bytes());
    ftyp_body.extend_from_slice(b"isomiso2avc1mp41");
    let ftyp = wrap_box(b"ftyp", &ftyp_body);

    let timescale = 1000u32;
    let dur_units = (duration_seconds.max(0.1) * f64::from(timescale)).round() as u32;

    let vcodec = VideoCodec::from_name(&video_info.codec_name).unwrap_or(VideoCodec::H264);
    let v_fourcc = match vcodec {
        VideoCodec::H265 => *b"hvc1",
        VideoCodec::Vp9 => *b"vp09",
        VideoCodec::Av1 => *b"av01",
        _ => *b"avc1",
    };
    let acodec = AudioCodec::from_name(&audio_info.codec_name).unwrap_or(AudioCodec::Aac);
    let a_fourcc = match acodec {
        AudioCodec::Mp3 => *b".mp3",
        AudioCodec::Opus => *b"Opus",
        AudioCodec::Flac => *b"fLaC",
        AudioCodec::PcmS16Le => *b"lpcm",
        _ => *b"mp4a",
    };

    let total_payload_len = video_payload_len + audio_payload_len;
    let mdat_hdr_len = if total_payload_len > u32::MAX as u64 {
        16u64
    } else {
        8u64
    };

    // Build dummy moov to get exact moov size
    let dummy_video_trak = build_video_trak(
        1,
        timescale,
        dur_units,
        video_info.width.unwrap_or(1280),
        video_info.height.unwrap_or(720),
        v_fourcc,
        video_payload_len.min(u32::MAX as u64) as u32,
        0,
        None,
    );
    let dummy_audio_trak = build_audio_trak(
        2,
        timescale,
        dur_units,
        audio_info.sample_rate.unwrap_or(48000),
        audio_info.channels.unwrap_or(2),
        a_fourcc,
        audio_payload_len.min(u32::MAX as u64) as u32,
        0,
    );
    let dummy_mvhd = build_mvhd(timescale, dur_units, 3);
    let mut dummy_moov_body = dummy_mvhd;
    dummy_moov_body.extend_from_slice(&dummy_video_trak);
    dummy_moov_body.extend_from_slice(&dummy_audio_trak);
    let dummy_moov = wrap_box(b"moov", &dummy_moov_body);

    let video_chunk_offset = ftyp.len() as u64 + dummy_moov.len() as u64 + mdat_hdr_len;
    let audio_chunk_offset = video_chunk_offset + video_payload_len;

    let aux = TrackAuxBoxes {
        colr: None,
        mdcv: video_info.mastering_display.clone(),
        clli: video_info.content_light_level.clone(),
        sv3d: video_info.spatial_360.clone(),
    };

    let real_video_trak = build_video_trak(
        1,
        timescale,
        dur_units,
        video_info.width.unwrap_or(1280),
        video_info.height.unwrap_or(720),
        v_fourcc,
        video_payload_len.min(u32::MAX as u64) as u32,
        video_chunk_offset,
        Some(&aux),
    );
    let real_audio_trak = build_audio_trak(
        2,
        timescale,
        dur_units,
        audio_info.sample_rate.unwrap_or(48000),
        audio_info.channels.unwrap_or(2),
        a_fourcc,
        audio_payload_len.min(u32::MAX as u64) as u32,
        audio_chunk_offset,
    );
    let real_mvhd = build_mvhd(timescale, dur_units, 3);
    let mut real_moov_body = real_mvhd;
    real_moov_body.extend_from_slice(&real_video_trak);
    real_moov_body.extend_from_slice(&real_audio_trak);
    let moov = wrap_box(b"moov", &real_moov_body);

    writer.write_all(&ftyp)?;
    writer.write_all(&moov)?;

    // Write mdat header
    if mdat_hdr_len == 16 {
        let total = total_payload_len + 16;
        let mut hdr = [0u8; 16];
        hdr[0..4].copy_from_slice(&1u32.to_be_bytes());
        hdr[4..8].copy_from_slice(b"mdat");
        hdr[8..16].copy_from_slice(&total.to_be_bytes());
        writer.write_all(&hdr)?;
    } else {
        let total = (total_payload_len + 8) as u32;
        let mut hdr = [0u8; 8];
        hdr[0..4].copy_from_slice(&total.to_be_bytes());
        hdr[4..8].copy_from_slice(b"mdat");
        writer.write_all(&hdr)?;
    }

    // Stream video payload
    video_reader.seek(SeekFrom::Start(video_payload_offset))?;
    copy_chunked(video_reader, writer, video_payload_len, 64 * 1024)?;

    // Stream audio payload
    audio_reader.seek(SeekFrom::Start(audio_payload_offset))?;
    copy_chunked(audio_reader, writer, audio_payload_len, 64 * 1024)?;

    Ok(())
}

/// Copies `total_bytes` from `reader` to `writer` in bounded chunks (O(1) memory).
pub fn copy_chunked<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    total_bytes: u64,
    chunk_size: usize,
) -> RuviResult<()> {
    let mut remaining = total_bytes;
    let mut buf = vec![0u8; chunk_size.clamp(4096, 1024 * 1024)];
    while remaining > 0 {
        let to_read = remaining.min(buf.len() as u64) as usize;
        reader.read_exact(&mut buf[..to_read])?;
        writer.write_all(&buf[..to_read])?;
        remaining -= to_read as u64;
    }
    Ok(())
}

/// Strictly validates the structural integrity of a complete `moov` box (header included).
///
/// Checks, without allocating copies of the tree:
/// * every child box size fits inside its parent (container boxes are walked recursively),
/// * visual sample-entry child boxes (`colr`, `mdcv`, `clli`, `sv3d`, ...) fit inside the entry,
/// * `stco`/`co64` declared entry counts match the box body length,
/// * every chunk offset lies strictly before `file_len`.
///
/// Any violation yields [`RuviError::Integrity`] (exit code 8).
pub fn validate_moov_integrity(moov_box: &[u8], file_len: u64) -> RuviResult<()> {
    let (box_type, start, end) = read_box_header(moov_box, 0)?;
    if &box_type != b"moov" {
        return Err(RuviError::Integrity(
            "expected 'moov' box at recorded offset".to_string(),
        ));
    }
    validate_children(&moov_box[start..end], file_len, 0)
}

const MAX_BOX_DEPTH: usize = 16;

fn validate_children(body: &[u8], file_len: u64, depth: usize) -> RuviResult<()> {
    if depth > MAX_BOX_DEPTH {
        return Err(RuviError::Integrity("MP4 box nesting too deep".to_string()));
    }
    let mut pos = 0usize;
    while pos < body.len() {
        if body.len() - pos < 8 {
            return Err(RuviError::Integrity(
                "trailing bytes shorter than a box header".to_string(),
            ));
        }
        let (box_type, start, end) = read_box_header(body, pos)?;
        let payload = &body[start..end];
        match &box_type {
            b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts" | b"dinf" | b"udta" | b"mvex" => {
                validate_children(payload, file_len, depth + 1)?;
            }
            b"stsd" => validate_stsd(payload, file_len, depth + 1)?,
            b"stco" => validate_chunk_offsets(payload, 4, file_len)?,
            b"co64" => validate_chunk_offsets(payload, 8, file_len)?,
            _ => {}
        }
        pos = end;
    }
    Ok(())
}

fn validate_stsd(payload: &[u8], file_len: u64, depth: usize) -> RuviResult<()> {
    if payload.len() < 8 {
        return Err(RuviError::Integrity("truncated 'stsd' box".to_string()));
    }
    let count = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
    let mut pos = 8usize;
    for _ in 0..count {
        let (fourcc, start, end) = read_box_header(payload, pos)?;
        let entry = &payload[start..end];
        let children_at = match &fourcc {
            b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"vp09" | b"vp08" | b"av01" | b"mp4v" => {
                Some(78usize)
            }
            b"mp4a" | b"Opus" | b"opus" | b"fLaC" | b"ac-3" | b"ec-3" => Some(28usize),
            _ => None,
        };
        if let Some(offset) = children_at
            && entry.len() > offset
        {
            validate_children(&entry[offset..], file_len, depth + 1)?;
        }
        pos = end;
    }
    if pos > payload.len() {
        return Err(RuviError::Integrity(
            "'stsd' entries overflow box".to_string(),
        ));
    }
    Ok(())
}

fn validate_chunk_offsets(payload: &[u8], width: usize, file_len: u64) -> RuviResult<()> {
    if payload.len() < 8 {
        return Err(RuviError::Integrity(
            "truncated chunk offset box".to_string(),
        ));
    }
    if payload[0..4] != [0, 0, 0, 0] {
        return Err(RuviError::Integrity(
            "chunk offset box has non-zero version/flags".to_string(),
        ));
    }
    let count = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
    let needed = count
        .checked_mul(width)
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| RuviError::Integrity("chunk offset count overflow".to_string()))?;
    if needed != payload.len() {
        return Err(RuviError::Integrity(format!(
            "chunk offset table declares {count} entries but box holds {} bytes",
            payload.len()
        )));
    }
    for raw in payload[8..].chunks_exact(width) {
        let offset = if width == 4 {
            u64::from(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]))
        } else {
            u64::from_be_bytes(raw.try_into().unwrap_or([0u8; 8]))
        };
        if offset >= file_len {
            return Err(RuviError::Integrity(format!(
                "chunk offset {offset} points past end of file ({file_len} bytes)"
            )));
        }
    }
    Ok(())
}
