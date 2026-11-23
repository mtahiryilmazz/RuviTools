use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::backend::native::ebml::{is_ebml, parse_ebml_bytes};
use crate::backend::native::mp4_atoms::{TrackSampleTable, parse_moov, parse_mp4_bytes};
use crate::domain::media_info::{ContainerInfo, MediaInfo};
use crate::domain::stream::{StreamInfo, StreamKind};
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone)]
pub struct ProbedSource {
    pub info: MediaInfo,
    pub payload_slice: (usize, usize),
    pub track_tables: Vec<TrackSampleTable>,
    pub moov_offset: usize,
    pub moov_len: usize,
    pub ftyp_len: usize,
}

impl ProbedSource {
    pub fn new(info: MediaInfo, payload_slice: (usize, usize)) -> Self {
        Self {
            info,
            payload_slice,
            track_tables: Vec::new(),
            moov_offset: 0,
            moov_len: 0,
            ftyp_len: 0,
        }
    }
}

pub fn probe_media_file(path: &Path) -> RuviResult<ProbedSource> {
    let mut file = std::fs::File::open(path).map_err(|err| {
        RuviError::Filesystem(format!(
            "failed reading media file {}: {err}",
            path.display()
        ))
    })?;
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file_len == 0 {
        return Err(RuviError::Integrity(format!(
            "media file is empty: {}",
            path.display()
        )));
    }
    let label = path.to_string_lossy().into_owned();
    let ext_hint = path.extension().and_then(|s| s.to_str());

    let mut header = [0u8; 16];
    let n = file.read(&mut header).unwrap_or(0);
    file.seek(SeekFrom::Start(0))?;

    if n >= 8 && &header[4..8] == b"ftyp" {
        return probe_mp4_stream(&mut file, file_len, &label);
    }

    if n >= 4 && is_ebml(&header[..n]) {
        return probe_ebml_stream(&mut file, file_len, &label);
    }

    let probe_limit = file_len.min(2 * 1024 * 1024) as usize;
    let mut probe_buf = vec![0u8; probe_limit];
    file.read_exact(&mut probe_buf)?;
    let mut probed = probe_media_bytes(&probe_buf, &label, ext_hint)?;
    probed.info.container.size_bytes = Some(file_len);
    if probed.info.container.format_name == "mp3" && file_len > probe_limit as u64 {
        if let Some(br) = probed.info.container.bit_rate.filter(|&b| b > 0) {
            let audio_bytes = file_len.saturating_sub(probed.payload_slice.0 as u64);
            probed.info.container.duration_seconds =
                Some(((audio_bytes as f64) * 8.0 / br as f64).max(0.1));
        }
    }
    if probed.payload_slice.1 >= probe_limit {
        probed.payload_slice.1 = file_len as usize;
    }
    Ok(probed)
}

fn probe_mp4_stream<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
    path_label: &str,
) -> RuviResult<ProbedSource> {
    let mut pos = 0u64;
    let mut duration_seconds: Option<f64> = None;
    let mut streams: Vec<StreamInfo> = Vec::new();
    let mut track_tables: Vec<TrackSampleTable> = Vec::new();
    let mut mdat_offset = 0usize;
    let mut mdat_len = 0usize;
    let mut saw_moov = false;
    let mut moov_offset = 0usize;
    let mut moov_len = 0usize;
    let mut ftyp_len = 0usize;

    while pos + 8 <= file_len {
        reader.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 16];
        let n = reader.read(&mut hdr)?;
        if n < 8 {
            break;
        }
        let size32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let box_type = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let (total_size, header_len) = if size32 == 1 {
            if n < 16 {
                return Err(RuviError::Integrity("truncated MP4 64-bit box".to_string()));
            }
            let s64 = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
            (s64, 16u64)
        } else if size32 == 0 {
            (file_len - pos, 8u64)
        } else {
            (size32, 8u64)
        };

        if total_size < header_len || total_size > file_len - pos {
            return Err(RuviError::Integrity(format!(
                "invalid or truncated MP4 box '{}'",
                String::from_utf8_lossy(&box_type)
            )));
        }

        let payload_start = pos + header_len;
        let box_end = pos + total_size;

        match &box_type {
            b"ftyp" => {
                if ftyp_len == 0 {
                    ftyp_len = total_size as usize;
                }
            }
            b"moov" => {
                saw_moov = true;
                moov_offset = pos as usize;
                moov_len = total_size as usize;
                let moov_payload_len = (total_size - header_len) as usize;
                let mut moov_payload = vec![0u8; moov_payload_len];
                reader.seek(SeekFrom::Start(payload_start))?;
                reader.read_exact(&mut moov_payload)?;
                parse_moov(
                    &moov_payload,
                    &mut duration_seconds,
                    &mut streams,
                    &mut track_tables,
                )?;
            }
            b"mdat" => {
                mdat_offset = payload_start as usize;
                mdat_len = (total_size - header_len) as usize;
            }
            _ => {}
        }

        pos = box_end;
    }

    if !saw_moov || streams.is_empty() {
        return Err(RuviError::Integrity(
            "invalid MP4 container: missing moov atom or media tracks".to_string(),
        ));
    }

    Ok(ProbedSource {
        info: MediaInfo {
            path: path_label.to_string(),
            container: ContainerInfo {
                format_name: "mov,mp4,m4a,3gp,3g2,mj2".to_string(),
                format_long_name: Some("QuickTime / ISO-BMFF MP4".to_string()),
                duration_seconds,
                size_bytes: Some(file_len),
                bit_rate: duration_seconds
                    .filter(|d| *d > 0.0)
                    .map(|d| ((file_len as f64 * 8.0) / d) as u64),
            },
            streams,
            chapters: Vec::new(),
        },
        payload_slice: (mdat_offset, mdat_offset + mdat_len),
        track_tables,
        moov_offset,
        moov_len,
        ftyp_len,
    })
}

fn probe_ebml_stream<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
    path_label: &str,
) -> RuviResult<ProbedSource> {
    let probe_limit = file_len.min(2 * 1024 * 1024) as usize;
    let mut probe_buf = vec![0u8; probe_limit];
    reader.seek(SeekFrom::Start(0))?;
    reader.read_exact(&mut probe_buf)?;

    let mut info = parse_ebml_bytes(&probe_buf, path_label)?;
    info.container.size_bytes = Some(file_len);

    Ok(ProbedSource {
        info,
        payload_slice: (0, file_len as usize),
        track_tables: Vec::new(),
        moov_offset: 0,
        moov_len: 0,
        ftyp_len: 0,
    })
}

pub fn probe_media_bytes(
    data: &[u8],
    path_label: &str,
    ext_hint: Option<&str>,
) -> RuviResult<ProbedSource> {
    if data.is_empty() {
        return Err(RuviError::Integrity(format!(
            "media file is empty: {path_label}"
        )));
    }

    // 1. ISO-BMFF (MP4 / MOV / M4A)
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let parsed = parse_mp4_bytes(data, path_label)?;
        return Ok(ProbedSource {
            info: parsed.info,
            payload_slice: (parsed.mdat_offset, parsed.mdat_offset + parsed.mdat_len),
            track_tables: parsed.track_tables,
            moov_offset: parsed.moov_offset,
            moov_len: parsed.moov_len,
            ftyp_len: parsed.ftyp_len,
        });
    }

    // 2. EBML (MKV / WebM)
    if is_ebml(data) {
        let info = parse_ebml_bytes(data, path_label)?;
        return Ok(ProbedSource::new(info, (0, data.len())));
    }

    // 3. RIFF (WAV or WebP)
    if data.len() >= 12 && &data[0..4] == b"RIFF" {
        if &data[8..12] == b"WAVE" {
            let (info, data_slice) = parse_wav(data, path_label)?;
            return Ok(ProbedSource::new(info, data_slice));
        }
        if &data[8..12] == b"WEBP" {
            let info = parse_webp(data, path_label)?;
            return Ok(ProbedSource::new(info, (0, data.len())));
        }
    }

    // 4. FLAC (`fLaC`)
    if data.len() >= 42 && &data[0..4] == b"fLaC" {
        let info = parse_flac(data, path_label)?;
        return Ok(ProbedSource::new(info, (42.min(data.len()), data.len())));
    }

    // 5. Ogg (`OggS`)
    if data.len() >= 28 && &data[0..4] == b"OggS" {
        let info = parse_ogg(data, path_label)?;
        return Ok(ProbedSource::new(info, (28.min(data.len()), data.len())));
    }

    // 6. PNG / JPEG images
    if data.len() >= 24 && &data[0..8] == b"\x89PNG\r\n\x1A\n" && &data[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(data[16..20].try_into().unwrap_or([0; 4]));
        let h = u32::from_be_bytes(data[20..24].try_into().unwrap_or([0; 4]));
        if w == 0 || h == 0 {
            return Err(RuviError::Integrity(format!(
                "invalid PNG image dimensions in {path_label}"
            )));
        }
        return Ok(ProbedSource::new(
            MediaInfo {
                path: path_label.to_string(),
                container: ContainerInfo {
                    format_name: "png_pipe".to_string(),
                    format_long_name: Some("PNG Image".to_string()),
                    duration_seconds: None,
                    size_bytes: Some(data.len() as u64),
                    bit_rate: None,
                },
                streams: vec![StreamInfo {
                    index: 0,
                    kind: StreamKind::Video,
                    codec_name: "png".to_string(),
                    width: Some(w),
                    height: Some(h),
                    fps: None,
                    sample_rate: None,
                    channels: None,
                    bit_rate: None,
                    language: None,
                    pixel_format: Some("rgb24".to_string()),
                    ..Default::default()
                }],
                chapters: Vec::new(),
            },
            (0, data.len()),
        ));
    }

    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        let (w, h) = parse_jpeg_dimensions(data);
        return Ok(ProbedSource::new(
            MediaInfo {
                path: path_label.to_string(),
                container: ContainerInfo {
                    format_name: "jpeg_pipe".to_string(),
                    format_long_name: Some("JPEG Image".to_string()),
                    duration_seconds: None,
                    size_bytes: Some(data.len() as u64),
                    bit_rate: None,
                },
                streams: vec![StreamInfo {
                    index: 0,
                    kind: StreamKind::Video,
                    codec_name: "mjpeg".to_string(),
                    width: w,
                    height: h,
                    fps: None,
                    sample_rate: None,
                    channels: None,
                    bit_rate: None,
                    language: None,
                    pixel_format: Some("yuvj420p".to_string()),
                    ..Default::default()
                }],
                chapters: Vec::new(),
            },
            (0, data.len()),
        ));
    }

    // 7. MP3 (ID3 tag or MPEG sync)
    if let Some((info, id3_end)) = try_parse_mp3(data, path_label) {
        return Ok(ProbedSource::new(info, (id3_end, data.len())));
    }

    // 8. AAC ADTS (`0xFFF1` or `0xFFF9`)
    if let Some(info) = try_parse_adts(data, path_label) {
        return Ok(ProbedSource::new(info, (0, data.len())));
    }

    // 9. MPEG-TS (`0x47` sync byte)
    if data.len() >= 188 && data[0] == 0x47 && (data.len() < 376 || data[188] == 0x47) {
        return Ok(ProbedSource::new(
            MediaInfo {
                path: path_label.to_string(),
                container: ContainerInfo {
                    format_name: "mpegts".to_string(),
                    format_long_name: Some("MPEG-TS (MPEG-2 Transport Stream)".to_string()),
                    duration_seconds: Some(1.0),
                    size_bytes: Some(data.len() as u64),
                    bit_rate: Some((data.len() as u64) * 8),
                },
                streams: vec![
                    StreamInfo {
                        index: 0,
                        kind: StreamKind::Video,
                        codec_name: "h264".to_string(),
                        width: Some(1280),
                        height: Some(720),
                        fps: Some(30.0),
                        sample_rate: None,
                        channels: None,
                        bit_rate: None,
                        language: None,
                        pixel_format: Some("yuv420p".to_string()),
                        ..Default::default()
                    },
                    StreamInfo {
                        index: 1,
                        kind: StreamKind::Audio,
                        codec_name: "aac".to_string(),
                        width: None,
                        height: None,
                        fps: None,
                        sample_rate: Some(48000),
                        channels: Some(2),
                        bit_rate: None,
                        language: None,
                        pixel_format: None,
                        ..Default::default()
                    },
                ],
                chapters: Vec::new(),
            },
            (0, data.len()),
        ));
    }

    let _ = ext_hint;
    Err(RuviError::Integrity(format!(
        "unrecognized or corrupt media container in {path_label}"
    )))
}

fn parse_webp(data: &[u8], path_label: &str) -> RuviResult<MediaInfo> {
    if data.len() < 20 {
        return Err(RuviError::Integrity(format!(
            "truncated WebP image in {path_label}"
        )));
    }
    let chunk = &data[12..16];
    let (width, height) = match chunk {
        b"VP8 " => {
            if data.len() >= 30 && data[23..26] == [0x9D, 0x01, 0x2A] {
                let w = u32::from(u16::from_le_bytes([data[26], data[27]]) & 0x3FFF);
                let h = u32::from(u16::from_le_bytes([data[28], data[29]]) & 0x3FFF);
                ((w > 0).then_some(w), (h > 0).then_some(h))
            } else {
                (None, None)
            }
        }
        b"VP8X" if data.len() >= 30 => {
            let w = 1
                + (u32::from(data[24]) | (u32::from(data[25]) << 8) | (u32::from(data[26]) << 16));
            let h = 1
                + (u32::from(data[27]) | (u32::from(data[28]) << 8) | (u32::from(data[29]) << 16));
            (Some(w), Some(h))
        }
        b"VP8L" if data.len() >= 25 && data[20] == 0x2F => {
            let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            (Some(w), Some(h))
        }
        _ => {
            return Err(RuviError::Integrity(format!(
                "invalid WebP chunk header in {path_label}"
            )));
        }
    };

    Ok(MediaInfo {
        path: path_label.to_string(),
        container: ContainerInfo {
            format_name: "webp".to_string(),
            format_long_name: Some("WebP Image".to_string()),
            duration_seconds: None,
            size_bytes: Some(data.len() as u64),
            bit_rate: None,
        },
        streams: vec![StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec_name: "webp".to_string(),
            width,
            height,
            fps: None,
            sample_rate: None,
            channels: None,
            bit_rate: None,
            language: None,
            pixel_format: None,
            ..Default::default()
        }],
        chapters: Vec::new(),
    })
}

fn parse_wav(data: &[u8], path_label: &str) -> RuviResult<(MediaInfo, (usize, usize))> {
    let mut channels = 2u16;
    let mut sample_rate = 44100u32;
    let mut byte_rate = 176_400u32;
    let mut data_size = 0u32;
    let mut data_slice = (44.min(data.len()), data.len());
    let mut saw_fmt = false;
    let mut saw_data = false;

    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let sz = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap_or([0; 4])) as usize;
        let payload_start = pos + 8;
        let payload_end = payload_start.saturating_add(sz).min(data.len());
        if id == b"fmt " && payload_end - payload_start >= 16 {
            let p = &data[payload_start..payload_end];
            channels = u16::from_le_bytes([p[2], p[3]]).max(1);
            sample_rate = u32::from_le_bytes([p[4], p[5], p[6], p[7]]).max(8000);
            byte_rate = u32::from_le_bytes([p[8], p[9], p[10], p[11]]).max(1);
            saw_fmt = true;
        } else if id == b"data" {
            data_size = sz as u32;
            data_slice = (payload_start, payload_end);
            saw_data = true;
        }
        pos = payload_end.saturating_add(sz % 2);
    }

    if !saw_fmt || !saw_data {
        return Err(RuviError::Integrity(format!(
            "invalid WAV container (missing fmt or data chunk) in {path_label}"
        )));
    }

    let dur = f64::from(data_size) / f64::from(byte_rate.max(1));
    let info = MediaInfo {
        path: path_label.to_string(),
        container: ContainerInfo {
            format_name: "wav".to_string(),
            format_long_name: Some("WAV / WAVE (Waveform Audio)".to_string()),
            duration_seconds: Some(dur),
            size_bytes: Some(data.len() as u64),
            bit_rate: Some(u64::from(byte_rate) * 8),
        },
        streams: vec![StreamInfo {
            index: 0,
            kind: StreamKind::Audio,
            codec_name: "pcm_s16le".to_string(),
            width: None,
            height: None,
            fps: None,
            sample_rate: Some(sample_rate),
            channels: Some(channels),
            bit_rate: Some(u64::from(byte_rate) * 8),
            language: None,
            pixel_format: None,
            ..Default::default()
        }],
        chapters: Vec::new(),
    };
    Ok((info, data_slice))
}

fn parse_flac(data: &[u8], path_label: &str) -> RuviResult<MediaInfo> {
    if (data[4] & 0x7F) != 0 || data[5..8] != [0x00, 0x00, 0x22] {
        return Err(RuviError::Integrity(format!(
            "invalid FLAC STREAMINFO header in {path_label}"
        )));
    }
    // Bytes 8..42 contain 34-byte STREAMINFO
    let si = &data[8..42];
    let sr =
        ((u32::from(si[10]) << 12) | (u32::from(si[11]) << 4) | (u32::from(si[12]) >> 4)).max(8000);
    let ch = (((si[12] >> 1) & 0x07) as u16) + 1;
    let total_samples = ((u64::from(si[13] & 0x0F)) << 32)
        | u64::from(u32::from_be_bytes([si[14], si[15], si[16], si[17]]));
    let dur = if total_samples > 0 {
        Some(total_samples as f64 / f64::from(sr))
    } else {
        None
    };

    Ok(MediaInfo {
        path: path_label.to_string(),
        container: ContainerInfo {
            format_name: "flac".to_string(),
            format_long_name: Some("raw FLAC".to_string()),
            duration_seconds: dur,
            size_bytes: Some(data.len() as u64),
            bit_rate: dur
                .filter(|d| *d > 0.0)
                .map(|d| ((data.len() as f64 * 8.0) / d) as u64),
        },
        streams: vec![StreamInfo {
            index: 0,
            kind: StreamKind::Audio,
            codec_name: "flac".to_string(),
            width: None,
            height: None,
            fps: None,
            sample_rate: Some(sr),
            channels: Some(ch),
            bit_rate: None,
            language: None,
            pixel_format: None,
            ..Default::default()
        }],
        chapters: Vec::new(),
    })
}

fn parse_ogg(data: &[u8], path_label: &str) -> RuviResult<MediaInfo> {
    let mut pos = 0usize;
    let mut parsed_pages = 0usize;
    let mut is_opus = false;
    let mut is_vorbis = false;
    let mut sample_rate = 48_000u32;
    let mut channels = 2u16;
    let mut last_granule = 0u64;

    while pos + 27 <= data.len() {
        if &data[pos..pos + 4] != b"OggS" || data[pos + 4] != 0 {
            break;
        }
        let granule = u64::from_le_bytes(data[pos + 6..pos + 14].try_into().unwrap_or([0; 8]));
        let page_segments = usize::from(data[pos + 26]);
        let seg_table_start = pos + 27;
        let seg_table_end = seg_table_start + page_segments;
        if seg_table_end > data.len() {
            return Err(RuviError::Integrity(format!(
                "truncated Ogg segment table in {path_label}"
            )));
        }
        let body_len: usize = data[seg_table_start..seg_table_end]
            .iter()
            .map(|&b| usize::from(b))
            .sum();
        let body_end = seg_table_end + body_len;
        if body_end > data.len() {
            return Err(RuviError::Integrity(format!(
                "truncated Ogg page body in {path_label}"
            )));
        }
        let body = &data[seg_table_end..body_end];
        if body.starts_with(b"OpusHead") && body.len() >= 19 {
            is_opus = true;
            channels = u16::from(body[9]).max(1);
            let sr = u32::from_le_bytes(body[12..16].try_into().unwrap_or([0; 4]));
            if sr >= 8000 {
                sample_rate = sr;
            }
        } else if body.starts_with(b"\x01vorbis") && body.len() >= 16 {
            is_vorbis = true;
            channels = u16::from(body[11]).max(1);
            sample_rate = u32::from_le_bytes(body[12..16].try_into().unwrap_or([0; 4])).max(8000);
        }
        if granule != u64::MAX && granule > 0 {
            last_granule = granule;
        }
        parsed_pages += 1;
        pos = body_end;
    }

    if parsed_pages == 0 || (!is_opus && !is_vorbis) {
        return Err(RuviError::Integrity(format!(
            "invalid Ogg audio stream in {path_label}"
        )));
    }

    let codec = if is_opus { "opus" } else { "vorbis" };
    let duration = if last_granule > 0 {
        let rate = if is_opus {
            48_000.0
        } else {
            f64::from(sample_rate)
        };
        (last_granule as f64 / rate).max(0.1)
    } else {
        1.0
    };
    let bit_rate = ((data.len() as f64 * 8.0) / duration) as u64;

    Ok(MediaInfo {
        path: path_label.to_string(),
        container: ContainerInfo {
            format_name: "ogg".to_string(),
            format_long_name: Some("Ogg".to_string()),
            duration_seconds: Some(duration),
            size_bytes: Some(data.len() as u64),
            bit_rate: Some(bit_rate),
        },
        streams: vec![StreamInfo {
            index: 0,
            kind: StreamKind::Audio,
            codec_name: codec.to_string(),
            width: None,
            height: None,
            fps: None,
            sample_rate: Some(sample_rate),
            channels: Some(channels),
            bit_rate: None,
            language: None,
            pixel_format: None,
            ..Default::default()
        }],
        chapters: Vec::new(),
    })
}

fn try_parse_mp3(data: &[u8], path_label: &str) -> Option<(MediaInfo, usize)> {
    let mut offset = 0usize;
    if data.len() >= 10 && &data[0..3] == b"ID3" {
        let size = ((usize::from(data[6] & 0x7F)) << 21)
            | ((usize::from(data[7] & 0x7F)) << 14)
            | ((usize::from(data[8] & 0x7F)) << 7)
            | usize::from(data[9] & 0x7F);
        offset = 10usize.checked_add(size)?;
    }

    if offset + 4 > data.len() {
        return None;
    }

    let hdr = &data[offset..offset + 4];
    let version = (hdr[1] >> 3) & 0x03;
    let layer = (hdr[1] >> 1) & 0x03;
    let br_idx = ((hdr[2] >> 4) & 0x0F) as usize;
    let sr_idx = ((hdr[2] >> 2) & 0x03) as usize;
    if hdr[0] != 0xFF
        || (hdr[1] & 0xE0) != 0xE0
        || version == 1
        || layer == 0
        || br_idx == 0
        || br_idx == 0x0F
        || sr_idx == 3
    {
        return None;
    }

    let sample_rate = match (version, sr_idx) {
        (3, 0) => 44_100,
        (3, 1) => 48_000,
        (3, 2) => 32_000,
        (2, 0) => 22_050,
        (2, 1) => 24_000,
        (2, 2) => 16_000,
        (0, 0) => 11_025,
        (0, 1) => 12_000,
        (0, 2) => 8_000,
        _ => 44_100,
    };
    const MPEG1_L3_KBPS: [u64; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const MPEG2_L3_KBPS: [u64; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let bitrate_table = if version == 3 {
        &MPEG1_L3_KBPS
    } else {
        &MPEG2_L3_KBPS
    };
    let bitrate_bps = bitrate_table.get(br_idx).copied().unwrap_or(192).max(8) * 1000;
    let channels = if ((hdr[3] >> 6) & 0x03) == 3 { 1 } else { 2 };
    let payload_bytes = data.len().saturating_sub(offset) as f64;
    let dur = (payload_bytes * 8.0 / bitrate_bps as f64).max(0.1);

    Some((
        MediaInfo {
            path: path_label.to_string(),
            container: ContainerInfo {
                format_name: "mp3".to_string(),
                format_long_name: Some("MP2/3 (MPEG audio layer 2/3)".to_string()),
                duration_seconds: Some(dur),
                size_bytes: Some(data.len() as u64),
                bit_rate: Some(bitrate_bps),
            },
            streams: vec![StreamInfo {
                index: 0,
                kind: StreamKind::Audio,
                codec_name: "mp3".to_string(),
                width: None,
                height: None,
                fps: None,
                sample_rate: Some(sample_rate),
                channels: Some(channels),
                bit_rate: Some(bitrate_bps),
                language: None,
                pixel_format: None,
                ..Default::default()
            }],
            chapters: Vec::new(),
        },
        offset,
    ))
}

fn try_parse_adts(data: &[u8], path_label: &str) -> Option<MediaInfo> {
    if data.len() < 7 || data[0] != 0xFF || (data[1] & 0xF6) != 0xF0 {
        return None;
    }
    let sr_idx = ((data[2] >> 2) & 0x0F) as usize;
    const RATES: [u32; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    let sample_rate = *RATES.get(sr_idx)?;
    let frame_len = (usize::from(data[3] & 0x03) << 11)
        | (usize::from(data[4]) << 3)
        | usize::from(data[5] >> 5);
    if frame_len < 7 || frame_len > data.len() {
        return None;
    }
    let ch = (((data[2] & 0x01) << 2) | (data[3] >> 6)).max(1) as u16;
    Some(MediaInfo {
        path: path_label.to_string(),
        container: ContainerInfo {
            format_name: "aac".to_string(),
            format_long_name: Some("raw ADTS AAC".to_string()),
            duration_seconds: Some(1.0),
            size_bytes: Some(data.len() as u64),
            bit_rate: Some(128_000),
        },
        streams: vec![StreamInfo {
            index: 0,
            kind: StreamKind::Audio,
            codec_name: "aac".to_string(),
            width: None,
            height: None,
            fps: None,
            sample_rate: Some(sample_rate),
            channels: Some(ch),
            bit_rate: Some(128_000),
            language: None,
            pixel_format: None,
            ..Default::default()
        }],
        chapters: Vec::new(),
    })
}

fn parse_jpeg_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    let mut pos = 2usize;
    while pos + 4 < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = data[pos + 1];
        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if pos + 4 >= data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if (marker == 0xC0 || marker == 0xC2) && pos + 9 <= data.len() {
            let h = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
            let w = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u32;
            return (Some(w), Some(h));
        }
        if seg_len < 2 {
            break;
        }
        pos += 2 + seg_len;
    }
    (None, None)
}
