use std::io::Cursor;

use crate::domain::media_info::{ContainerInfo, MediaInfo};
use crate::domain::stream::{AudioCodec, ContainerFormat, StreamInfo, StreamKind, VideoCodec};
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone)]
pub struct ParsedMp4 {
    pub info: MediaInfo,
    pub mdat_offset: usize,
    pub mdat_len: usize,
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
    let mut mdat_offset = 0usize;
    let mut mdat_len = 0usize;
    let mut saw_moov = false;

    while pos + 8 <= data.len() {
        let (box_type, payload_start, box_end) = read_box_header(data, pos)?;
        let payload = &data[payload_start..box_end];
        match &box_type {
            b"moov" => {
                saw_moov = true;
                parse_moov(payload, &mut duration_seconds, &mut streams)?;
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
    })
}

fn read_box_header(data: &[u8], pos: usize) -> RuviResult<([u8; 4], usize, usize)> {
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

fn parse_moov(
    moov: &[u8],
    duration_seconds: &mut Option<f64>,
    streams: &mut Vec<StreamInfo>,
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
                if let Some(stream) = parse_trak(payload, streams.len()) {
                    streams.push(stream);
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

fn parse_trak(trak: &[u8], index: usize) -> Option<StreamInfo> {
    let mut tkhd_dims: (Option<u32>, Option<u32>) = (None, None);
    let mut handler_type = [0u8; 4];
    let mut language: Option<String> = None;
    let mut stsd_codec: Option<String> = None;
    let mut stsd_dims: (Option<u32>, Option<u32>) = (None, None);
    let mut sample_rate: Option<u32> = None;
    let mut channels: Option<u16> = None;

    let mut pos = 0usize;
    while pos + 8 <= trak.len() {
        let Ok((btype, pstart, end)) = read_box_header(trak, pos) else {
            break;
        };
        let payload = &trak[pstart..end];
        if &btype == b"tkhd" {
            tkhd_dims = parse_tkhd_dimensions(payload);
        } else if &btype == b"mdia" {
            parse_mdia(
                payload,
                &mut handler_type,
                &mut language,
                &mut stsd_codec,
                &mut stsd_dims,
                &mut sample_rate,
                &mut channels,
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

    Some(StreamInfo {
        index,
        kind,
        codec_name,
        width,
        height,
        fps: (kind == StreamKind::Video).then_some(30.0),
        sample_rate: if kind == StreamKind::Audio {
            sample_rate
        } else {
            None
        },
        channels: if kind == StreamKind::Audio {
            channels
        } else {
            None
        },
        bit_rate: None,
        language,
        pixel_format: (kind == StreamKind::Video).then(|| "yuv420p".to_string()),
    })
}

fn parse_tkhd_dimensions(payload: &[u8]) -> (Option<u32>, Option<u32>) {
    let min_len = if payload.first() == Some(&1) { 96 } else { 84 };
    if payload.len() < min_len {
        return (None, None);
    }
    let w = u16::from_be_bytes([payload[min_len - 8], payload[min_len - 7]]) as u32;
    let h = u16::from_be_bytes([payload[min_len - 4], payload[min_len - 3]]) as u32;
    ((w > 0).then_some(w), (h > 0).then_some(h))
}

fn parse_mdia(
    mdia: &[u8],
    handler_type: &mut [u8; 4],
    language: &mut Option<String>,
    codec: &mut Option<String>,
    dims: &mut (Option<u32>, Option<u32>),
    sample_rate: &mut Option<u32>,
    channels: &mut Option<u16>,
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
                parse_minf(payload, codec, dims, sample_rate, channels);
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
                if &stype == b"stsd" {
                    parse_stsd(&payload[spstart..send], codec, dims, sample_rate, channels);
                }
                spos = send;
            }
        }
        pos = end;
    }
}

fn parse_stsd(
    stsd: &[u8],
    codec: &mut Option<String>,
    dims: &mut (Option<u32>, Option<u32>),
    sample_rate: &mut Option<u32>,
    channels: &mut Option<u16>,
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

    let mut traks = Vec::new();
    let mut next_track_id = 1u32;

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
            spec.mdat_payload.len().max(16) as u32,
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
            spec.mdat_payload.len().max(16) as u32,
        ));
        next_track_id += 1;
    }

    let mvhd = build_mvhd(timescale, duration_units, next_track_id);
    let mut moov_body = mvhd;
    for t in traks {
        moov_body.extend_from_slice(&t);
    }
    let moov = wrap_box(b"moov", &moov_body);

    let payload = if spec.mdat_payload.is_empty() {
        &b"RUVIC_MEDIA_DATA"[..]
    } else {
        spec.mdat_payload
    };
    let mdat = wrap_box(b"mdat", payload);

    let mut out = Vec::with_capacity(ftyp.len() + moov.len() + mdat.len());
    out.extend_from_slice(&ftyp);
    out.extend_from_slice(&moov);
    out.extend_from_slice(&mdat);
    out
}

fn wrap_box(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
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
    // rate = 1.0 (0x00010000), volume = 1.0 (0x0100)
    b[20..24].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[24..26].copy_from_slice(&0x0100u16.to_be_bytes());
    // identity matrix
    b[36..40].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[52..56].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    b[68..72].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    b[96..100].copy_from_slice(&next_track_id.to_be_bytes());
    wrap_box(b"mvhd", &b)
}

fn build_tkhd(track_id: u32, duration: u32, width: u32, height: u32, is_audio: bool) -> Vec<u8> {
    let mut b = vec![0u8; 84];
    b[3] = 0x07; // flags: enabled | in_movie | in_preview
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
    // "und" packed = 0x55C4
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
) -> Vec<u8> {
    let tkhd = build_tkhd(track_id, duration, width, height, false);
    let mdhd = build_mdhd(timescale, duration);
    let hdlr = build_hdlr(b"vide", b"VideoHandler");

    // VisualSampleEntry
    let mut entry = vec![0u8; 78];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes()); // data_reference_index
    entry[24..26].copy_from_slice(&(width as u16).to_be_bytes());
    entry[26..28].copy_from_slice(&(height as u16).to_be_bytes());
    entry[28..32].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    entry[32..36].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    entry[40..42].copy_from_slice(&1u16.to_be_bytes());
    entry[74..76].copy_from_slice(&24u16.to_be_bytes());
    entry[76..78].copy_from_slice(&0xFFFFu16.to_be_bytes());
    let sample_entry = wrap_box(&fourcc, &entry);

    let mut stsd_body = vec![0u8; 8];
    stsd_body[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_body.extend_from_slice(&sample_entry);
    let stsd = wrap_box(b"stsd", &stsd_body);

    let stbl = build_minimal_stbl(stsd, duration, sample_size);
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
) -> Vec<u8> {
    let tkhd = build_tkhd(track_id, duration, 0, 0, true);
    let mdhd = build_mdhd(timescale, duration);
    let hdlr = build_hdlr(b"soun", b"SoundHandler");

    // AudioSampleEntry (28 bytes body)
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

    let stbl = build_minimal_stbl(stsd, duration, sample_size);
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

fn build_minimal_stbl(stsd: Vec<u8>, duration: u32, sample_size: u32) -> Vec<u8> {
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

    let mut stco = vec![0u8; 12];
    stco[4..8].copy_from_slice(&1u32.to_be_bytes());
    stco[8..12].copy_from_slice(&0u32.to_be_bytes());

    let mut body = stsd;
    body.extend_from_slice(&wrap_box(b"stts", &stts));
    body.extend_from_slice(&wrap_box(b"stsc", &stsc));
    body.extend_from_slice(&wrap_box(b"stsz", &stsz));
    body.extend_from_slice(&wrap_box(b"stco", &stco));
    let _ = Cursor::new(&body);
    wrap_box(b"stbl", &body)
}
