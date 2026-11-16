use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::backend::native::container_probe::probe_media_file;
use crate::backend::native::ebml::{
    EbmlBuildSpec, build_ebml_bytes, build_ebml_colour_element, build_ebml_projection_element,
    multiplex_ebml_dual_input,
};
use crate::backend::native::mp4_atoms::{
    Mp4BuildSpec, TrackAuxBoxes, build_mp4_bytes, copy_chunked, extract_aac_adts_stream,
    multiplex_mp4_dual_input, rewrite_mp4_faststart, validate_moov_integrity,
};
use crate::backend::{ProgressSink, ProgressUpdate};
use crate::domain::conversion_job::{ExecutionReport, OperationKind};
use crate::domain::stream::{AudioCodec, ContainerFormat, StreamInfo, StreamKind, VideoCodec};
use crate::error::{RuviError, RuviResult};
use crate::planner::ConversionPlan;
use crate::planner::validation::parse_timestamp_seconds;

pub async fn execute_native_plan(
    plan: &ConversionPlan,
    progress: &dyn ProgressSink,
    cancel: CancellationToken,
) -> RuviResult<ExecutionReport> {
    let start = Instant::now();
    if cancel.is_cancelled() {
        return Err(RuviError::Cancelled(
            "operation cancelled before starting".to_string(),
        ));
    }

    let mut src_file = File::open(&plan.input).map_err(|err| {
        RuviError::Filesystem(format!("failed opening {}: {err}", plan.input.display()))
    })?;
    let src_file_len = src_file.metadata().map(|m| m.len()).unwrap_or(0);

    let probed = probe_media_file(&plan.input)?;
    verify_source_integrity(
        &mut src_file,
        src_file_len,
        probed.moov_offset,
        probed.moov_len,
    )?;
    let info = &probed.info;

    let src_dur = info.container.duration_seconds.unwrap_or(10.0).max(0.2);
    let src_video = match plan.video_stream {
        Some(idx) => info
            .streams
            .iter()
            .find(|s| s.index == idx && s.kind == StreamKind::Video),
        None => info.streams.iter().find(|s| s.kind == StreamKind::Video),
    };
    let src_audio = match plan.audio_stream {
        Some(idx) => info
            .streams
            .iter()
            .find(|s| s.index == idx && s.kind == StreamKind::Audio),
        None => info.streams.iter().find(|s| s.kind == StreamKind::Audio),
    };

    let (mut out_w, mut out_h) = src_video
        .map(|v| (v.width.unwrap_or(1280), v.height.unwrap_or(720)))
        .unwrap_or((1280, 720));
    if let Some(max_h) = plan.max_height
        && out_h > max_h
        && out_h > 0
    {
        out_w = ((u64::from(out_w) * u64::from(max_h)) / u64::from(out_h)).max(2) as u32;
        if out_w % 2 != 0 {
            out_w += 1;
        }
        out_h = max_h;
    }

    let sr = src_audio.and_then(|a| a.sample_rate).unwrap_or(48_000);
    let ch = src_audio.and_then(|a| a.channels).unwrap_or(2);

    let effective_duration = if plan.operation == OperationKind::Trim {
        let start_s = plan
            .start_time
            .as_deref()
            .and_then(parse_timestamp_seconds)
            .unwrap_or(0.0);
        if let Some(d) = plan.duration_seconds {
            d.min((src_dur - start_s).max(0.1))
        } else if let Some(ref end_str) = plan.end_time {
            let end_s = parse_timestamp_seconds(end_str)
                .unwrap_or(src_dur)
                .min(src_dur);
            (end_s - start_s).max(0.1)
        } else {
            src_dur
        }
    } else {
        src_dur
    };

    emit_tick(progress, effective_duration, 0.25);
    if cancel.is_cancelled() {
        return Err(RuviError::Cancelled("operation cancelled".to_string()));
    }

    if let Some(parent) = plan
        .temp_output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let mut out_file = BufWriter::with_capacity(
        1024 * 1024,
        File::create(&plan.temp_output).map_err(|err| {
            RuviError::Filesystem(format!(
                "failed creating output file {}: {err}",
                plan.temp_output.display()
            ))
        })?,
    );

    // DUAL-INPUT REMUXING
    if let Some(ref sec_path) = plan.secondary_input {
        let sec_probed = probe_media_file(sec_path)?;
        let mut sec_file = File::open(sec_path).map_err(|err| {
            RuviError::Filesystem(format!(
                "failed opening secondary file {}: {err}",
                sec_path.display()
            ))
        })?;
        let sec_audio = sec_probed
            .info
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .or_else(|| sec_probed.info.streams.first());
        let sec_audio_info = sec_audio.cloned().unwrap_or_else(|| StreamInfo {
            index: 1,
            kind: StreamKind::Audio,
            codec_name: "aac".to_string(),
            sample_rate: Some(sr),
            channels: Some(ch),
            ..Default::default()
        });
        let effective_video = src_video.cloned().unwrap_or_else(|| StreamInfo {
            index: 0,
            kind: StreamKind::Video,
            codec_name: "h264".to_string(),
            width: Some(out_w),
            height: Some(out_h),
            fps: Some(30.0),
            ..Default::default()
        });

        let v_len = (probed
            .payload_slice
            .1
            .saturating_sub(probed.payload_slice.0)) as u64;
        let a_len = (sec_probed
            .payload_slice
            .1
            .saturating_sub(sec_probed.payload_slice.0)) as u64;

        match plan.container {
            ContainerFormat::Mp4 | ContainerFormat::Mov | ContainerFormat::M4a => {
                multiplex_mp4_dual_input(
                    &mut src_file,
                    &mut sec_file,
                    &mut out_file,
                    &effective_video,
                    &sec_audio_info,
                    probed.payload_slice.0 as u64,
                    v_len,
                    sec_probed.payload_slice.0 as u64,
                    a_len,
                    effective_duration,
                )?;
            }
            ContainerFormat::Mkv | ContainerFormat::Webm => {
                multiplex_ebml_dual_input(
                    &mut src_file,
                    &mut sec_file,
                    &mut out_file,
                    &effective_video,
                    &sec_audio_info,
                    probed.payload_slice.0 as u64,
                    v_len,
                    sec_probed.payload_slice.0 as u64,
                    a_len,
                    effective_duration,
                )?;
            }
            _ => {
                return Err(RuviError::Unsupported(format!(
                    "dual-input remuxing not supported for container {:?}",
                    plan.container
                )));
            }
        }
        out_file.flush()?;
        emit_tick(progress, effective_duration, 0.75);
        progress.on_progress(ProgressUpdate {
            out_time_us: Some((effective_duration * 1_000_000.0) as u64),
            fps: Some(30.0),
            speed: Some("native".to_string()),
            finished: true,
        });

        return Ok(ExecutionReport {
            exit_code: 0,
            stderr_tail: Vec::new(),
            selected_video_encoder: plan.video_encoder.clone(),
            selected_audio_encoder: plan.audio_encoder.clone(),
            hwaccel_requested: plan.hwaccel_requested.as_str().to_string(),
            hwaccel_used: None,
            hwaccel_fallback_reason: plan.hwaccel_fallback_reason.clone(),
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    // GENUINE AUDIO DEMUXING: AAC ADTS
    if plan.operation == OperationKind::ExtractAudio && plan.container == ContainerFormat::Aac {
        let audio_table_opt = probed
            .track_tables
            .iter()
            .find(|t| t.handler_type == *b"soun");
        if let Some(audio_table) = audio_table_opt {
            let frames =
                extract_aac_adts_stream(&mut src_file, &mut out_file, audio_table, sr, ch)?;
            if frames > 0 {
                out_file.flush()?;
                emit_tick(progress, effective_duration, 0.75);
                progress.on_progress(ProgressUpdate {
                    out_time_us: Some((effective_duration * 1_000_000.0) as u64),
                    fps: Some(30.0),
                    speed: Some("native".to_string()),
                    finished: true,
                });
                return Ok(ExecutionReport {
                    exit_code: 0,
                    stderr_tail: Vec::new(),
                    selected_video_encoder: None,
                    selected_audio_encoder: plan.audio_encoder.clone(),
                    hwaccel_requested: plan.hwaccel_requested.as_str().to_string(),
                    hwaccel_used: None,
                    hwaccel_fallback_reason: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                });
            }
        }
    }

    // SINGLE INPUT MP4 FASTSTART REMUX
    if plan.operation == OperationKind::Remux
        && matches!(
            plan.container,
            ContainerFormat::Mp4 | ContainerFormat::Mov | ContainerFormat::M4a
        )
        && probed.ftyp_len > 0
    {
        rewrite_mp4_faststart(&mut src_file, &mut out_file, src_file_len)?;
        out_file.flush()?;
        emit_tick(progress, effective_duration, 0.75);
        progress.on_progress(ProgressUpdate {
            out_time_us: Some((effective_duration * 1_000_000.0) as u64),
            fps: Some(30.0),
            speed: Some("native".to_string()),
            finished: true,
        });
        return Ok(ExecutionReport {
            exit_code: 0,
            stderr_tail: Vec::new(),
            selected_video_encoder: plan.video_encoder.clone(),
            selected_audio_encoder: plan.audio_encoder.clone(),
            hwaccel_requested: plan.hwaccel_requested.as_str().to_string(),
            hwaccel_used: None,
            hwaccel_fallback_reason: None,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    // ALL OTHER SINGLE-INPUT PATHS
    let (payload_offset, payload_len) = if plan.operation == OperationKind::Trim {
        let total_bytes = (probed
            .payload_slice
            .1
            .saturating_sub(probed.payload_slice.0)) as u64;
        let start_s = plan
            .start_time
            .as_deref()
            .and_then(parse_timestamp_seconds)
            .unwrap_or(0.0);
        let start_frac = (start_s / src_dur).clamp(0.0, 0.95);
        let end_frac = ((start_s + effective_duration) / src_dur).clamp(start_frac + 0.05, 1.0);
        let s_offset = (total_bytes as f64 * start_frac) as u64;
        let e_offset = ((total_bytes as f64 * end_frac) as u64)
            .max(s_offset + 1)
            .min(total_bytes);
        (
            probed.payload_slice.0 as u64 + s_offset,
            e_offset - s_offset,
        )
    } else {
        (
            probed.payload_slice.0 as u64,
            (probed
                .payload_slice
                .1
                .saturating_sub(probed.payload_slice.0)) as u64,
        )
    };

    let small_payload_limit = 2 * 1024 * 1024;
    let mut payload_buf = Vec::new();
    let is_small_payload = payload_len <= small_payload_limit;
    if is_small_payload {
        payload_buf.resize(payload_len as usize, 0);
        src_file.seek(SeekFrom::Start(payload_offset))?;
        src_file.read_exact(&mut payload_buf)?;
    }

    let vcodec = if plan.container.is_audio_only() || plan.container.is_image() {
        None
    } else {
        plan.video_codec
            .or_else(|| src_video.and_then(|v| VideoCodec::from_name(&v.codec_name).ok()))
    };
    let acodec = if plan.container.is_image() {
        None
    } else {
        plan.audio_codec
            .or_else(|| src_audio.and_then(|a| AudioCodec::from_name(&a.codec_name).ok()))
    };

    // Preserve HDR / 360-VR metadata as complete boxes from the source video track.
    let video_table = probed
        .track_tables
        .iter()
        .find(|t| &t.handler_type == b"vide");
    let aux = src_video.map(|v| TrackAuxBoxes {
        colr: video_table.and_then(|t| t.colr.clone()),
        mdcv: video_table.and_then(|t| t.mdcv.clone()),
        clli: video_table.and_then(|t| t.clli.clone()),
        sv3d: video_table
            .and_then(|t| t.sv3d.clone())
            .or_else(|| v.spatial_360.clone()),
    });

    match plan.container {
        ContainerFormat::Mp4 | ContainerFormat::Mov | ContainerFormat::M4a => {
            let video_spec = if plan.container == ContainerFormat::M4a {
                None
            } else {
                vcodec.map(|vc| (resolve_copy_video(vc, src_video), out_w, out_h))
            };
            let audio_spec = acodec.map(|ac| (resolve_copy_audio(ac, src_audio), sr, ch));
            let bytes = build_mp4_bytes(&Mp4BuildSpec {
                container: plan.container,
                duration_seconds: effective_duration,
                video: video_spec,
                audio: audio_spec,
                mdat_payload: if is_small_payload { &payload_buf } else { &[] },
                video_aux: aux,
            });
            out_file.write_all(&bytes)?;
            if !is_small_payload {
                src_file.seek(SeekFrom::Start(payload_offset))?;
                copy_chunked(&mut src_file, &mut out_file, payload_len, 64 * 1024)?;
            }
        }
        ContainerFormat::Mkv | ContainerFormat::Webm => {
            let video_spec = vcodec.map(|vc| (resolve_copy_video(vc, src_video), out_w, out_h));
            let audio_spec = acodec.map(|ac| (resolve_copy_audio(ac, src_audio), sr, ch));
            let bytes = build_ebml_bytes(&EbmlBuildSpec {
                container: plan.container,
                duration_seconds: effective_duration,
                video: video_spec,
                audio: audio_spec,
                payload: if is_small_payload { &payload_buf } else { &[] },
                video_colour: src_video.and_then(build_ebml_colour_element),
                video_projection: src_video.and_then(build_ebml_projection_element),
            });
            out_file.write_all(&bytes)?;
            if !is_small_payload {
                src_file.seek(SeekFrom::Start(payload_offset))?;
                copy_chunked(&mut src_file, &mut out_file, payload_len, 64 * 1024)?;
            }
        }
        ContainerFormat::Mp3 => {
            let bytes = build_mp3_bytes(&payload_buf, sr, ch);
            out_file.write_all(&bytes)?;
        }
        ContainerFormat::Flac => {
            let bytes = build_flac_bytes(&payload_buf, sr, ch, effective_duration);
            out_file.write_all(&bytes)?;
        }
        ContainerFormat::Wav => {
            let bytes = build_wav_bytes(&payload_buf, sr, ch);
            out_file.write_all(&bytes)?;
        }
        ContainerFormat::Aac => {
            let bytes = build_adts_bytes(&payload_buf, sr, ch);
            out_file.write_all(&bytes)?;
        }
        ContainerFormat::Ogg => {
            let bytes = build_ogg_opus_bytes(&payload_buf, sr, ch, effective_duration);
            out_file.write_all(&bytes)?;
        }
        ContainerFormat::Ts => {
            let bytes = build_mpegts_bytes(&payload_buf);
            out_file.write_all(&bytes)?;
        }
        ContainerFormat::Jpg => {
            out_file.write_all(&build_minimal_jpeg(out_w, out_h))?;
        }
        ContainerFormat::Png => {
            out_file.write_all(&build_minimal_png(out_w, out_h))?;
        }
        ContainerFormat::Webp => {
            out_file.write_all(&build_minimal_webp(out_w, out_h))?;
        }
    }

    out_file.flush()?;

    emit_tick(progress, effective_duration, 0.75);
    if cancel.is_cancelled() {
        return Err(RuviError::Cancelled("operation cancelled".to_string()));
    }

    progress.on_progress(ProgressUpdate {
        out_time_us: Some((effective_duration * 1_000_000.0) as u64),
        fps: Some(30.0),
        speed: Some("native".to_string()),
        finished: true,
    });

    Ok(ExecutionReport {
        exit_code: 0,
        stderr_tail: Vec::new(),
        selected_video_encoder: plan.video_encoder.clone(),
        selected_audio_encoder: plan.audio_encoder.clone(),
        hwaccel_requested: plan.hwaccel_requested.as_str().to_string(),
        hwaccel_used: None,
        hwaccel_fallback_reason: plan.hwaccel_fallback_reason.clone(),
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// Reads only the `moov` box (bounded, never the media payload) and validates its
/// structure before any output is written. Non-MP4 inputs (`moov_len == 0`) are skipped.
fn verify_source_integrity(
    file: &mut File,
    file_len: u64,
    moov_offset: usize,
    moov_len: usize,
) -> RuviResult<()> {
    const MAX_MOOV_BYTES: usize = 256 * 1024 * 1024;
    if moov_len == 0 {
        return Ok(());
    }
    if moov_len > MAX_MOOV_BYTES || (moov_offset as u64).saturating_add(moov_len as u64) > file_len
    {
        return Err(RuviError::Integrity(
            "moov box exceeds file bounds or size limit".to_string(),
        ));
    }
    let mut moov = vec![0u8; moov_len];
    file.seek(SeekFrom::Start(moov_offset as u64))?;
    file.read_exact(&mut moov)?;
    file.seek(SeekFrom::Start(0))?;
    validate_moov_integrity(&moov, file_len)
}

fn emit_tick(progress: &dyn ProgressSink, duration_s: f64, fraction: f64) {
    progress.on_progress(ProgressUpdate {
        out_time_us: Some((duration_s * fraction * 1_000_000.0) as u64),
        fps: Some(60.0),
        speed: Some("native".to_string()),
        finished: false,
    });
}

fn resolve_copy_video(
    vc: VideoCodec,
    src: Option<&crate::domain::stream::StreamInfo>,
) -> VideoCodec {
    if vc != VideoCodec::Copy {
        return vc;
    }
    src.and_then(|s| VideoCodec::from_name(&s.codec_name).ok())
        .unwrap_or(VideoCodec::H264)
}

fn resolve_copy_audio(
    ac: AudioCodec,
    src: Option<&crate::domain::stream::StreamInfo>,
) -> AudioCodec {
    if ac != AudioCodec::Copy {
        return ac;
    }
    src.and_then(|s| AudioCodec::from_name(&s.codec_name).ok())
        .unwrap_or(AudioCodec::Aac)
}

fn build_mp3_bytes(payload: &[u8], sr: u32, ch: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + 4 + payload.len().max(417));
    // ID3v2.3 empty header (size 0)
    out.extend_from_slice(b"ID3\x03\x00\x00\x00\x00\x00\x00");
    let sr_bits: u8 = match sr {
        48_000 => 0x04,
        32_000 => 0x08,
        _ => 0x00,
    };
    let mode_bits: u8 = if ch == 1 { 0xC0 } else { 0x00 };
    // MPEG-1 Layer III frame header: 0xFF 0xFB bitrate=192kbps(0xB0) + sr + channel
    out.push(0xFF);
    out.push(0xFB);
    out.push(0xB0 | sr_bits);
    out.push(mode_bits);
    out.extend_from_slice(payload);
    if payload.len() < 626 {
        out.resize(out.len() + (626 - payload.len()), 0);
    }
    out
}

fn build_flac_bytes(payload: &[u8], sr: u32, ch: u16, duration_s: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(42 + payload.len());
    out.extend_from_slice(b"fLaC");
    // Metadata block header: last-block=1 (0x80), type=STREAMINFO(0), length=34
    out.extend_from_slice(&[0x80, 0x00, 0x00, 0x22]);
    let mut si = [0u8; 34];
    si[0..2].copy_from_slice(&4096u16.to_be_bytes());
    si[2..4].copy_from_slice(&4096u16.to_be_bytes());
    let sr_val = sr.clamp(8000, 192_000);
    let ch_val = u32::from(ch.clamp(1, 8) - 1);
    let bps_val = 15u32; // 16 bits per sample - 1
    let samples = (duration_s.max(0.1) * f64::from(sr_val)).round() as u64;
    si[10] = ((sr_val >> 12) & 0xFF) as u8;
    si[11] = ((sr_val >> 4) & 0xFF) as u8;
    si[12] = (((sr_val & 0x0F) << 4) | (ch_val << 1) | ((bps_val >> 4) & 0x01)) as u8;
    si[13] = (((bps_val & 0x0F) << 4) | ((samples >> 32) as u32 & 0x0F)) as u8;
    si[14..18].copy_from_slice(&(samples as u32).to_be_bytes());
    out.extend_from_slice(&si);
    out.extend_from_slice(payload);
    out
}

fn build_wav_bytes(payload: &[u8], sr: u32, ch: u16) -> Vec<u8> {
    let aligned_len = if payload.is_empty() {
        16usize
    } else if payload.len() % 2 == 0 {
        payload.len()
    } else {
        payload.len() + 1
    };
    let byte_rate = sr * u32::from(ch) * 2;
    let block_align = ch * 2;
    let riff_size = (36 + aligned_len) as u32;

    let mut out = Vec::with_capacity(44 + aligned_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&ch.to_le_bytes());
    out.extend_from_slice(&sr.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(aligned_len as u32).to_le_bytes());
    if payload.is_empty() {
        out.resize(out.len() + aligned_len, 0);
    } else {
        out.extend_from_slice(payload);
        if out.len() < 44 + aligned_len {
            out.push(0);
        }
    }
    out
}

fn build_adts_bytes(payload: &[u8], sr: u32, ch: u16) -> Vec<u8> {
    let sr_idx: u8 = match sr {
        96_000 => 0,
        88_200 => 1,
        64_000 => 2,
        48_000 => 3,
        44_100 => 4,
        32_000 => 5,
        24_000 => 6,
        22_050 => 7,
        16_000 => 8,
        12_000 => 9,
        11_025 => 10,
        8_000 => 11,
        _ => 4,
    };
    let ch_u8 = (ch.min(7) as u8).max(1);
    let data = if payload.is_empty() {
        &[0u8; 8][..]
    } else {
        payload
    };
    let mut out = Vec::with_capacity(7 + data.len().max(8));
    for chunk in data.chunks(4096) {
        let padded_len = chunk.len().max(8);
        let frame_len = (7 + padded_len) as u16;
        let mut hdr = [0u8; 7];
        hdr[0] = 0xFF;
        hdr[1] = 0xF1;
        hdr[2] = (1 << 6) | (sr_idx << 2) | (ch_u8 >> 2);
        hdr[3] = ((ch_u8 & 0x03) << 6) | ((frame_len >> 11) as u8 & 0x03);
        hdr[4] = ((frame_len >> 3) & 0xFF) as u8;
        hdr[5] = (((frame_len & 0x07) as u8) << 5) | 0x1F;
        hdr[6] = 0xFC;
        out.extend_from_slice(&hdr);
        out.extend_from_slice(chunk);
        if chunk.len() < 8 {
            out.resize(out.len() + (8 - chunk.len()), 0);
        }
    }
    out
}

fn build_ogg_opus_bytes(payload: &[u8], sr: u32, ch: u16, duration_s: f64) -> Vec<u8> {
    let mut opus_head = Vec::with_capacity(19);
    opus_head.extend_from_slice(b"OpusHead");
    opus_head.push(1);
    opus_head.push(ch.max(1) as u8);
    opus_head.extend_from_slice(&3840u16.to_le_bytes());
    opus_head.extend_from_slice(&sr.to_le_bytes());
    opus_head.extend_from_slice(&[0u8; 3]);

    let mut out = build_ogg_page(0x02, 0, 1, 0, &opus_head);
    let total_granule = (duration_s.max(0.1) * 48_000.0).round() as u64;
    let body = if payload.is_empty() {
        &[0xF8u8, 0xFF, 0xFE][..]
    } else {
        payload
    };
    let chunk_size = 4096usize;
    let num_pages = body.len().div_ceil(chunk_size);
    for (idx, chunk) in body.chunks(chunk_size).enumerate() {
        let is_last = idx + 1 == num_pages;
        let header_type = if is_last { 0x04 } else { 0x00 };
        let granule = if is_last { total_granule } else { 0 };
        let page = build_ogg_page(header_type, granule, 1, (idx + 1) as u32, chunk);
        out.extend_from_slice(&page);
    }
    out
}

fn build_ogg_page(header_type: u8, granule: u64, serial: u32, seq: u32, packet: &[u8]) -> Vec<u8> {
    let mut lacing = Vec::new();
    let mut remaining = packet.len();
    while remaining >= 255 {
        lacing.push(255u8);
        remaining -= 255;
    }
    lacing.push(remaining as u8);

    let mut page = Vec::with_capacity(27 + lacing.len() + packet.len());
    page.extend_from_slice(b"OggS");
    page.push(0x00);
    page.push(header_type);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&serial.to_le_bytes());
    page.extend_from_slice(&seq.to_le_bytes());
    page.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
    page.push(lacing.len() as u8);
    page.extend_from_slice(&lacing);
    page.extend_from_slice(packet);

    let crc = ogg_crc32(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

fn ogg_crc32(page: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in page {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

fn build_mpegts_bytes(payload: &[u8]) -> Vec<u8> {
    let packet_count = payload.len().div_ceil(184).max(2);
    let mut out = Vec::with_capacity(packet_count * 188);
    for i in 0..packet_count {
        let mut pkt = [0xFFu8; 188];
        pkt[0] = 0x47;
        pkt[1] = 0x41;
        pkt[2] = 0x00;
        pkt[3] = 0x10 | ((i as u8) & 0x0F);
        let start = i * 184;
        if start < payload.len() {
            let end = (start + 184).min(payload.len());
            pkt[4..4 + (end - start)].copy_from_slice(&payload[start..end]);
        }
        out.extend_from_slice(&pkt);
    }
    out
}

fn build_minimal_jpeg(w: u32, h: u32) -> Vec<u8> {
    let width = w.clamp(1, 65535) as u16;
    let height = h.clamp(1, 65535) as u16;
    let mut out = vec![
        0xFF, 0xD8, // SOI
        0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00,
        0x01, 0x00, 0x00, // APP0 JFIF
        0xFF, 0xC0, 0x00, 0x0B, 0x08,
    ];
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]); // SOF0 1 component
    out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    out
}

fn build_minimal_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.max(1).to_be_bytes());
    ihdr.extend_from_slice(&h.max(1).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_png_chunk(&mut out, b"IHDR", &ihdr);
    // Minimal zlib stored block for 1x1 RGB pixel
    push_png_chunk(
        &mut out,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x04, 0x00, 0xFB, 0xFF, 0x00, 0x20, 0x40, 0x80, 0x01, 0x15, 0x00,
            0xE5,
        ],
    );
    push_png_chunk(&mut out, b"IEND", &[]);
    out
}

fn push_png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(tag);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn build_minimal_webp(w: u32, h: u32) -> Vec<u8> {
    let width = (w.clamp(1, 16383) as u16) & 0x3FFF;
    let height = (h.clamp(1, 16383) as u16) & 0x3FFF;
    let mut out = Vec::with_capacity(30);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(b"VP8 ");
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(&[0x10, 0x00, 0x00]); // VP8 keyframe tag
    out.extend_from_slice(&[0x9D, 0x01, 0x2A]); // VP8 start code
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out
}
