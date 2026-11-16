use std::io::{Read, Seek, SeekFrom, Write};

use crate::backend::native::mp4_atoms::copy_chunked;
use crate::domain::media_info::{ContainerInfo, MediaInfo};
use crate::domain::stream::{AudioCodec, ContainerFormat, StreamInfo, StreamKind, VideoCodec};
use crate::error::{RuviError, RuviResult};

const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

pub fn is_ebml(data: &[u8]) -> bool {
    data.len() >= 4 && data[0..4] == EBML_MAGIC
}

pub fn parse_ebml_bytes(data: &[u8], path_label: &str) -> RuviResult<MediaInfo> {
    if !is_ebml(data) {
        return Err(RuviError::Integrity(
            "invalid EBML/Matroska file: missing EBML magic header".to_string(),
        ));
    }

    let mut doc_type = "matroska".to_string();
    let mut timestamp_scale: u64 = 1_000_000;
    let mut raw_duration: Option<f64> = None;
    let mut streams: Vec<StreamInfo> = Vec::new();
    let mut saw_segment = false;

    let mut pos = 0usize;
    while pos < data.len() {
        let Some((id, id_len)) = read_ebml_id(&data[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, unknown_size)) = read_ebml_size(&data[pos..]) else {
            break;
        };
        pos += sz_len;
        let elem_end = if unknown_size {
            data.len()
        } else {
            let Some(end) = u64::try_from(pos)
                .ok()
                .and_then(|p| p.checked_add(size))
                .and_then(|e| usize::try_from(e).ok())
            else {
                return Err(RuviError::Integrity(
                    "invalid EBML element size".to_string(),
                ));
            };
            if id != 0x1853_8067 && end > data.len() {
                return Err(RuviError::Integrity(
                    "truncated EBML element payload".to_string(),
                ));
            }
            end.min(data.len())
        };
        let payload = &data[pos..elem_end];

        match id {
            0x1A45_DFA3 => {
                if let Some(dt) = scan_ebml_doc_type(payload) {
                    doc_type = dt;
                }
            }
            0x1853_8067 => {
                saw_segment = true;
                parse_segment(
                    payload,
                    &mut timestamp_scale,
                    &mut raw_duration,
                    &mut streams,
                )?;
                break;
            }
            _ => {}
        }
        pos = elem_end;
    }

    if !saw_segment || streams.is_empty() {
        return Err(RuviError::Integrity(
            "invalid Matroska/WebM container: missing Segment or tracks".to_string(),
        ));
    }

    let duration_seconds = raw_duration.map(|d| (d * timestamp_scale as f64) / 1_000_000_000.0);
    let format_name = if doc_type == "webm" {
        "matroska,webm".to_string()
    } else {
        "matroska".to_string()
    };

    Ok(MediaInfo {
        path: path_label.to_string(),
        container: ContainerInfo {
            format_name,
            format_long_name: Some("Matroska / WebM (EBML)".to_string()),
            duration_seconds,
            size_bytes: Some(data.len() as u64),
            bit_rate: duration_seconds
                .filter(|d| *d > 0.0)
                .map(|d| ((data.len() as f64 * 8.0) / d) as u64),
        },
        streams,
        chapters: Vec::new(),
    })
}

fn scan_ebml_doc_type(header_payload: &[u8]) -> Option<String> {
    let mut pos = 0usize;
    while pos < header_payload.len() {
        let (id, id_len) = read_ebml_id(&header_payload[pos..])?;
        pos += id_len;
        let (size, sz_len, _) = read_ebml_size(&header_payload[pos..])?;
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(header_payload.len());
        if id == 0x4282 {
            return Some(
                String::from_utf8_lossy(&header_payload[pos..end])
                    .trim_end_matches('\0')
                    .to_string(),
            );
        }
        pos = end;
    }
    None
}

fn parse_segment(
    seg: &[u8],
    timestamp_scale: &mut u64,
    raw_duration: &mut Option<f64>,
    streams: &mut Vec<StreamInfo>,
) -> RuviResult<()> {
    let mut pos = 0usize;
    while pos < seg.len() {
        let Some((id, id_len)) = read_ebml_id(&seg[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, unknown)) = read_ebml_size(&seg[pos..]) else {
            break;
        };
        pos += sz_len;
        let end = if unknown {
            seg.len()
        } else {
            let end = usize::try_from(size)
                .ok()
                .and_then(|s| pos.checked_add(s))
                .ok_or_else(|| {
                    RuviError::Integrity("invalid EBML segment element size".to_string())
                })?;
            if (id == 0x1549_A966 || id == 0x1654_AE6B) && end > seg.len() {
                return Err(RuviError::Integrity(
                    "truncated EBML Info or Tracks element".to_string(),
                ));
            }
            end.min(seg.len())
        };
        let payload = &seg[pos..end];
        match id {
            0x1549_A966 => parse_info(payload, timestamp_scale, raw_duration),
            0x1654_AE6B => parse_tracks(payload, streams),
            0x1F43_B675 => break, // Stop at first Cluster
            _ => {}
        }
        pos = end;
    }
    Ok(())
}

fn parse_info(info: &[u8], timestamp_scale: &mut u64, raw_duration: &mut Option<f64>) {
    let mut pos = 0usize;
    while pos < info.len() {
        let Some((id, id_len)) = read_ebml_id(&info[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, _)) = read_ebml_size(&info[pos..]) else {
            break;
        };
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(info.len());
        let val = &info[pos..end];
        if id == 0x002A_D7B1 {
            if let Some(u) = read_uint_be(val) {
                *timestamp_scale = u;
            }
        } else if id == 0x4489 {
            *raw_duration = read_float_be(val);
        }
        pos = end;
    }
}

fn parse_tracks(tracks: &[u8], streams: &mut Vec<StreamInfo>) {
    let mut pos = 0usize;
    while pos < tracks.len() {
        let Some((id, id_len)) = read_ebml_id(&tracks[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, _)) = read_ebml_size(&tracks[pos..]) else {
            break;
        };
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(tracks.len());
        if id == 0xAE
            && let Some(s) = parse_track_entry(&tracks[pos..end], streams.len())
        {
            streams.push(s);
        }
        pos = end;
    }
}

fn parse_track_entry(entry: &[u8], fallback_idx: usize) -> Option<StreamInfo> {
    let mut pos = 0usize;
    let mut track_type: u64 = 0;
    let mut codec_id = String::new();
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut sample_rate: Option<u32> = None;
    let mut channels: Option<u16> = None;
    let mut language: Option<String> = None;
    let mut stream_info = StreamInfo::default();

    while pos < entry.len() {
        let (id, id_len) = read_ebml_id(&entry[pos..])?;
        pos += id_len;
        let (size, sz_len, _) = read_ebml_size(&entry[pos..])?;
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(entry.len());
        let val = &entry[pos..end];
        match id {
            0x83 => track_type = read_uint_be(val).unwrap_or(0),
            0x86 => {
                codec_id = String::from_utf8_lossy(val)
                    .trim_end_matches('\0')
                    .to_string()
            }
            0x0022_B59C => {
                language = Some(
                    String::from_utf8_lossy(val)
                        .trim_end_matches('\0')
                        .to_string(),
                );
            }
            0xE0 => parse_video_settings(val, &mut width, &mut height, &mut stream_info),
            0xE1 => parse_audio_settings(val, &mut sample_rate, &mut channels),
            _ => {}
        }
        pos = end;
    }

    let kind = match track_type {
        1 => StreamKind::Video,
        2 => StreamKind::Audio,
        17 => StreamKind::Subtitle,
        _ => return None,
    };

    stream_info.index = fallback_idx;
    stream_info.kind = kind;
    stream_info.codec_name = map_ebml_codec(&codec_id);
    stream_info.width = width;
    stream_info.height = height;
    stream_info.fps = (kind == StreamKind::Video).then_some(30.0);
    stream_info.sample_rate = sample_rate;
    stream_info.channels = channels;
    stream_info.language = language;
    if kind == StreamKind::Video && stream_info.pixel_format.is_none() {
        stream_info.pixel_format = Some("yuv420p".to_string());
    }

    Some(stream_info)
}

fn parse_video_settings(
    v: &[u8],
    width: &mut Option<u32>,
    height: &mut Option<u32>,
    stream: &mut StreamInfo,
) {
    let mut pos = 0usize;
    while pos < v.len() {
        let Some((id, id_len)) = read_ebml_id(&v[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, _)) = read_ebml_size(&v[pos..]) else {
            break;
        };
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(v.len());
        let val = &v[pos..end];
        if id == 0xB0 {
            *width = read_uint_be(val).map(|n| n as u32);
        } else if id == 0xBA {
            *height = read_uint_be(val).map(|n| n as u32);
        } else if id == 0x55B0 {
            parse_ebml_colour(val, stream);
        } else if id == 0x7670 {
            parse_ebml_projection(val, stream);
        }
        pos = end;
    }
}

fn parse_ebml_colour(c: &[u8], stream: &mut StreamInfo) {
    let mut pos = 0usize;
    while pos < c.len() {
        let Some((id, id_len)) = read_ebml_id(&c[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, _)) = read_ebml_size(&c[pos..]) else {
            break;
        };
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(c.len());
        let val = &c[pos..end];
        match id {
            0x55B1 => {
                stream.color_matrix = read_uint_be(val).map(|n| n as u16);
            }
            0x55B7 => {
                stream.color_primaries = read_uint_be(val).map(|n| n as u16);
            }
            0x55B8 => {
                stream.color_transfer = read_uint_be(val).map(|n| n as u16);
            }
            0x55B9 => {
                stream.color_full_range = read_uint_be(val).map(|n| n == 2);
            }
            0x55D0 => {
                stream.mastering_display = Some(val.to_vec());
            }
            _ => {}
        }
        pos = end;
    }
}

fn parse_ebml_projection(p: &[u8], stream: &mut StreamInfo) {
    stream.spatial_360 = Some(p.to_vec());
}

fn parse_audio_settings(a: &[u8], sample_rate: &mut Option<u32>, channels: &mut Option<u16>) {
    let mut pos = 0usize;
    while pos < a.len() {
        let Some((id, id_len)) = read_ebml_id(&a[pos..]) else {
            break;
        };
        pos += id_len;
        let Some((size, sz_len, _)) = read_ebml_size(&a[pos..]) else {
            break;
        };
        pos += sz_len;
        let end = pos.saturating_add(size as usize).min(a.len());
        let val = &a[pos..end];
        if id == 0xB5 {
            *sample_rate = read_float_be(val).map(|f| f.round() as u32);
        } else if id == 0x9F {
            *channels = read_uint_be(val).map(|n| n as u16);
        }
        pos = end;
    }
}

fn map_ebml_codec(id: &str) -> String {
    match id {
        "V_MPEG4/ISO/AVC" => "h264".to_string(),
        "V_MPEGH/ISO/HEVC" => "hevc".to_string(),
        "V_VP9" => "vp9".to_string(),
        "V_AV1" => "av1".to_string(),
        s if s.starts_with("A_AAC") => "aac".to_string(),
        "A_OPUS" => "opus".to_string(),
        "A_VORBIS" => "vorbis".to_string(),
        "A_FLAC" => "flac".to_string(),
        "A_MPEG/L3" => "mp3".to_string(),
        "A_PCM/INT/LIT" => "pcm_s16le".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn read_ebml_id(buf: &[u8]) -> Option<(u32, usize)> {
    let first = *buf.first()?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    if len > 4 || buf.len() < len {
        return None;
    }
    let mut val = 0u32;
    for &b in &buf[..len] {
        val = (val << 8) | u32::from(b);
    }
    Some((val, len))
}

fn read_ebml_size(buf: &[u8]) -> Option<(u64, usize, bool)> {
    let first = *buf.first()?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || buf.len() < len {
        return None;
    }
    let mask = (1u8 << (8 - len)).wrapping_sub(1);
    let mut val = u64::from(first & mask);
    let mut all_ones = (first & mask) == mask;
    for &b in &buf[1..len] {
        if b != 0xFF {
            all_ones = false;
        }
        val = (val << 8) | u64::from(b);
    }
    Some((val, len, all_ones))
}

fn read_uint_be(buf: &[u8]) -> Option<u64> {
    if buf.is_empty() || buf.len() > 8 {
        return None;
    }
    let mut v = 0u64;
    for &b in buf {
        v = (v << 8) | u64::from(b);
    }
    Some(v)
}

fn read_float_be(buf: &[u8]) -> Option<f64> {
    if buf.len() == 4 {
        Some(f64::from(f32::from_be_bytes(buf.try_into().ok()?)))
    } else if buf.len() == 8 {
        Some(f64::from_be_bytes(buf.try_into().ok()?))
    } else {
        None
    }
}

pub struct EbmlBuildSpec<'a> {
    pub container: ContainerFormat,
    pub duration_seconds: f64,
    pub video: Option<(VideoCodec, u32, u32)>,
    pub audio: Option<(AudioCodec, u32, u16)>,
    pub payload: &'a [u8],
    pub video_colour: Option<Vec<u8>>,
    pub video_projection: Option<Vec<u8>>,
}

pub fn build_ebml_bytes(spec: &EbmlBuildSpec<'_>) -> Vec<u8> {
    let doc_type = if spec.container == ContainerFormat::Webm {
        b"webm".as_slice()
    } else {
        b"matroska".as_slice()
    };

    let mut hdr_body = Vec::new();
    hdr_body.extend_from_slice(&ebml_uint_elem(0x4286, 1));
    hdr_body.extend_from_slice(&ebml_uint_elem(0x42F7, 1));
    hdr_body.extend_from_slice(&ebml_uint_elem(0x42F2, 4));
    hdr_body.extend_from_slice(&ebml_uint_elem(0x42F3, 8));
    hdr_body.extend_from_slice(&ebml_elem(0x4282, doc_type));
    hdr_body.extend_from_slice(&ebml_uint_elem(0x4287, 4));
    hdr_body.extend_from_slice(&ebml_uint_elem(0x4285, 2));
    let ebml_hdr = ebml_elem(0x1A45_DFA3, &hdr_body);

    let mut info_body = Vec::new();
    info_body.extend_from_slice(&ebml_uint_elem(0x002A_D7B1, 1_000_000));
    let dur_ms = (spec.duration_seconds.max(0.1) * 1000.0).to_be_bytes();
    info_body.extend_from_slice(&ebml_elem(0x4489, &dur_ms));
    info_body.extend_from_slice(&ebml_elem(0x4D80, b"ruvic-native"));
    let info_elem = ebml_elem(0x1549_A966, &info_body);

    let mut tracks_body = Vec::new();
    let mut track_num = 1u64;
    if let Some((vcodec, w, h)) = spec.video {
        let codec_id = match vcodec {
            VideoCodec::H265 => "V_MPEGH/ISO/HEVC",
            VideoCodec::Vp9 => "V_VP9",
            VideoCodec::Av1 => "V_AV1",
            VideoCodec::H264 | VideoCodec::Copy => "V_MPEG4/ISO/AVC",
        };
        let mut video_el = Vec::new();
        video_el.extend_from_slice(&ebml_uint_elem(0xB0, u64::from(w.max(2))));
        video_el.extend_from_slice(&ebml_uint_elem(0xBA, u64::from(h.max(2))));
        if let Some(ref c) = spec.video_colour {
            video_el.extend_from_slice(c);
        }
        if let Some(ref p) = spec.video_projection {
            video_el.extend_from_slice(p);
        }
        let mut te = Vec::new();
        te.extend_from_slice(&ebml_uint_elem(0xD7, track_num));
        te.extend_from_slice(&ebml_uint_elem(0x73C5, track_num));
        te.extend_from_slice(&ebml_uint_elem(0x83, 1));
        te.extend_from_slice(&ebml_elem(0x86, codec_id.as_bytes()));
        te.extend_from_slice(&ebml_elem(0xE0, &video_el));
        tracks_body.extend_from_slice(&ebml_elem(0xAE, &te));
        track_num += 1;
    }

    if let Some((acodec, sr, ch)) = spec.audio {
        let codec_id = match acodec {
            AudioCodec::Opus => "A_OPUS",
            AudioCodec::Vorbis => "A_VORBIS",
            AudioCodec::Flac => "A_FLAC",
            AudioCodec::Mp3 => "A_MPEG/L3",
            AudioCodec::PcmS16Le => "A_PCM/INT/LIT",
            _ => "A_AAC",
        };
        let mut audio_el = Vec::new();
        audio_el.extend_from_slice(&ebml_elem(0xB5, &(sr.max(8000) as f64).to_be_bytes()));
        audio_el.extend_from_slice(&ebml_uint_elem(0x9F, u64::from(ch.max(1))));
        let mut te = Vec::new();
        te.extend_from_slice(&ebml_uint_elem(0xD7, track_num));
        te.extend_from_slice(&ebml_uint_elem(0x73C5, track_num));
        te.extend_from_slice(&ebml_uint_elem(0x83, 2));
        te.extend_from_slice(&ebml_elem(0x86, codec_id.as_bytes()));
        te.extend_from_slice(&ebml_elem(0xE1, &audio_el));
        tracks_body.extend_from_slice(&ebml_elem(0xAE, &te));
    }
    let tracks_elem = ebml_elem(0x1654_AE6B, &tracks_body);

    let data_slice = if spec.payload.is_empty() {
        b"RUVIC_CLUSTER_DATA".as_slice()
    } else {
        spec.payload
    };
    let mut cluster_body = ebml_uint_elem(0xE7, 0);
    cluster_body.extend_from_slice(&ebml_elem(0xA3, data_slice));
    let cluster_elem = ebml_elem(0x1F43_B675, &cluster_body);

    let mut seg_body = info_elem;
    seg_body.extend_from_slice(&tracks_elem);
    seg_body.extend_from_slice(&cluster_elem);
    let segment = ebml_elem(0x1853_8067, &seg_body);

    let mut out = Vec::with_capacity(ebml_hdr.len() + segment.len());
    out.extend_from_slice(&ebml_hdr);
    out.extend_from_slice(&segment);
    out
}

fn ebml_elem(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let id_bytes = id.to_be_bytes();
    let start = id_bytes.iter().position(|&b| b != 0).unwrap_or(3);
    out.extend_from_slice(&id_bytes[start..]);
    out.extend_from_slice(&encode_vint_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn ebml_uint_elem(id: u32, val: u64) -> Vec<u8> {
    if val == 0 {
        return ebml_elem(id, &[0]);
    }
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    ebml_elem(id, &bytes[start..])
}

fn encode_vint_size(size: u64) -> Vec<u8> {
    if size < (1 << 7) - 1 {
        vec![0x80 | (size as u8)]
    } else if size < (1 << 14) - 1 {
        let v = (0x4000u16 | (size as u16)).to_be_bytes();
        v.to_vec()
    } else if size < (1 << 21) - 1 {
        let v = (0x20_0000u32 | (size as u32)).to_be_bytes();
        v[1..4].to_vec()
    } else if size < (1 << 28) - 1 {
        let v = (0x1000_0000u32 | (size as u32)).to_be_bytes();
        v.to_vec()
    } else {
        let mut v = size.to_be_bytes();
        v[0] |= 0x01;
        v.to_vec()
    }
}

pub fn build_ebml_colour_element(stream: &StreamInfo) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    if let Some(matrix) = stream.color_matrix {
        body.extend_from_slice(&ebml_uint_elem(0x55B1, u64::from(matrix)));
    }
    if let Some(primaries) = stream.color_primaries {
        body.extend_from_slice(&ebml_uint_elem(0x55B7, u64::from(primaries)));
    }
    if let Some(transfer) = stream.color_transfer {
        body.extend_from_slice(&ebml_uint_elem(0x55B8, u64::from(transfer)));
    }
    if let Some(full_range) = stream.color_full_range {
        body.extend_from_slice(&ebml_uint_elem(0x55B9, if full_range { 2 } else { 1 }));
    }
    if let Some(ref md) = stream.mastering_display {
        body.extend_from_slice(&ebml_elem(0x55D0, md));
    }
    if body.is_empty() {
        None
    } else {
        Some(ebml_elem(0x55B0, &body))
    }
}

pub fn build_ebml_projection_element(stream: &StreamInfo) -> Option<Vec<u8>> {
    stream.spatial_360.as_ref().map(|p| ebml_elem(0x7670, p))
}

/// Multiplexes separate video and audio input streams into a single synchronized WebM container with bounded streaming.
#[allow(clippy::too_many_arguments)]
pub fn multiplex_ebml_dual_input<R1: Read + Seek, R2: Read + Seek, W: Write>(
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
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&ebml_uint_elem(0x4286, 1));
    hdr.extend_from_slice(&ebml_uint_elem(0x42F7, 1));
    hdr.extend_from_slice(&ebml_uint_elem(0x42F2, 4));
    hdr.extend_from_slice(&ebml_uint_elem(0x42F3, 8));
    hdr.extend_from_slice(&ebml_elem(0x4282, b"webm"));
    hdr.extend_from_slice(&ebml_uint_elem(0x4287, 4));
    hdr.extend_from_slice(&ebml_uint_elem(0x4285, 2));
    let ebml_header = ebml_elem(0x1A45_DFA3, &hdr);
    writer.write_all(&ebml_header)?;

    let mut info_body = Vec::new();
    info_body.extend_from_slice(&ebml_uint_elem(0x002A_D7B1, 1_000_000));
    let dur_ms = duration_seconds.max(0.1) * 1000.0;
    info_body.extend_from_slice(&ebml_elem(0x4489, &dur_ms.to_be_bytes()));
    info_body.extend_from_slice(&ebml_elem(0x4D80, b"ruvic-pure-rust"));
    let info_elem = ebml_elem(0x1549_A966, &info_body);

    let mut tracks_body = Vec::new();

    let vcodec = VideoCodec::from_name(&video_info.codec_name).unwrap_or(VideoCodec::Vp9);
    let v_codec_id = match vcodec {
        VideoCodec::Vp9 => "V_VP9",
        VideoCodec::Av1 => "V_AV1",
        VideoCodec::H265 => "V_MPEGH/ISO/HEVC",
        _ => "V_MPEG4/ISO/AVC",
    };
    let mut video_el = Vec::new();
    video_el.extend_from_slice(&ebml_uint_elem(
        0xB0,
        u64::from(video_info.width.unwrap_or(1280).max(2)),
    ));
    video_el.extend_from_slice(&ebml_uint_elem(
        0xBA,
        u64::from(video_info.height.unwrap_or(720).max(2)),
    ));
    if let Some(colr) = build_ebml_colour_element(video_info) {
        video_el.extend_from_slice(&colr);
    }
    if let Some(proj) = build_ebml_projection_element(video_info) {
        video_el.extend_from_slice(&proj);
    }
    let mut te1 = Vec::new();
    te1.extend_from_slice(&ebml_uint_elem(0xD7, 1));
    te1.extend_from_slice(&ebml_uint_elem(0x73C5, 1));
    te1.extend_from_slice(&ebml_uint_elem(0x83, 1));
    te1.extend_from_slice(&ebml_elem(0x86, v_codec_id.as_bytes()));
    te1.extend_from_slice(&ebml_elem(0xE0, &video_el));
    tracks_body.extend_from_slice(&ebml_elem(0xAE, &te1));

    let acodec = AudioCodec::from_name(&audio_info.codec_name).unwrap_or(AudioCodec::Opus);
    let a_codec_id = match acodec {
        AudioCodec::Opus => "A_OPUS",
        AudioCodec::Vorbis => "A_VORBIS",
        AudioCodec::Flac => "A_FLAC",
        AudioCodec::Mp3 => "A_MPEG/L3",
        _ => "A_AAC",
    };
    let mut audio_el = Vec::new();
    audio_el.extend_from_slice(&ebml_elem(
        0xB5,
        &(audio_info.sample_rate.unwrap_or(48000).max(8000) as f64).to_be_bytes(),
    ));
    audio_el.extend_from_slice(&ebml_uint_elem(
        0x9F,
        u64::from(audio_info.channels.unwrap_or(2).max(1)),
    ));
    let mut te2 = Vec::new();
    te2.extend_from_slice(&ebml_uint_elem(0xD7, 2));
    te2.extend_from_slice(&ebml_uint_elem(0x73C5, 2));
    te2.extend_from_slice(&ebml_uint_elem(0x83, 2));
    te2.extend_from_slice(&ebml_elem(0x86, a_codec_id.as_bytes()));
    te2.extend_from_slice(&ebml_elem(0xE1, &audio_el));
    tracks_body.extend_from_slice(&ebml_elem(0xAE, &te2));

    let tracks_elem = ebml_elem(0x1654_AE6B, &tracks_body);

    let seg_id = [0x18, 0x53, 0x80, 0x67];
    let seg_size = [0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    writer.write_all(&seg_id)?;
    writer.write_all(&seg_size)?;
    writer.write_all(&info_elem)?;
    writer.write_all(&tracks_elem)?;

    let timecode_el = ebml_uint_elem(0xE7, 0);
    let cluster_hdr = [0x1F, 0x43, 0xB6, 0x75];
    writer.write_all(&cluster_hdr)?;

    let block1_hdr_len = 1 + encode_vint_size(4 + video_payload_len).len() + 4;
    let block2_hdr_len = 1 + encode_vint_size(4 + audio_payload_len).len() + 4;
    let cluster_data_size = timecode_el.len() as u64
        + block1_hdr_len as u64
        + video_payload_len
        + block2_hdr_len as u64
        + audio_payload_len;

    writer.write_all(&encode_vint_size(cluster_data_size))?;
    writer.write_all(&timecode_el)?;

    // Video block
    writer.write_all(&[0xA3])?;
    writer.write_all(&encode_vint_size(4 + video_payload_len))?;
    writer.write_all(&[0x81, 0x00, 0x00, 0x80])?;
    video_reader.seek(SeekFrom::Start(video_payload_offset))?;
    copy_chunked(video_reader, writer, video_payload_len, 64 * 1024)?;

    // Audio block
    writer.write_all(&[0xA3])?;
    writer.write_all(&encode_vint_size(4 + audio_payload_len))?;
    writer.write_all(&[0x82, 0x00, 0x00, 0x80])?;
    audio_reader.seek(SeekFrom::Start(audio_payload_offset))?;
    copy_chunked(audio_reader, writer, audio_payload_len, 64 * 1024)?;

    Ok(())
}
