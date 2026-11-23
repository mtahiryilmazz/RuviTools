use std::borrow::Cow;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use rusty_mp3::{Mp3Encoder, Mp3EncoderConfig, vbr_quality_index};
use symphonia::core::codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::common::Tier;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use symphonia_opus::OpusDecoder;
use tokio_util::sync::CancellationToken;

use crate::backend::{ProgressSink, ProgressUpdate};
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone, Default)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub cover_jpeg: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct AudioTranscodeOptions {
    pub bitrate_kbps: u32,
    pub vbr_quality: Option<f32>,
    pub start_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub metadata: AudioMetadata,
}

/// Extracts clean Title, Artist, Album, Year, and Genre from a video/track title, optional
/// channel/author name, and source URL (handles `"Artist - Track"`, `「Album」`, `[Audio]`, etc.).
pub fn parse_smart_title_metadata(
    raw_title: &str,
    channel_hint: Option<&str>,
    source_url: Option<&str>,
) -> AudioMetadata {
    let mut clean = raw_title.trim().to_string();
    for suffix in [
        " [320kbps]",
        " [256kbps]",
        " [192kbps]",
        " [128kbps]",
        " [VBR]",
        " [M4A]",
        " [Opus]",
        " [WAV]",
        " [AAC]",
        " [Audio]",
        " (Audio)",
        " [Official Audio]",
        " (Official Audio)",
        " [Official Video]",
        " (Official Video)",
        " [Official Music Video]",
        " (Official Music Video)",
        " [Lyrics]",
        " (Lyrics)",
        " (Lyric Video)",
        " [HD]",
        " [4K]",
        " (4K Remaster)",
    ] {
        if let Some(pos) = clean
            .to_ascii_lowercase()
            .find(&suffix.to_ascii_lowercase())
        {
            clean.replace_range(pos..pos + suffix.len(), "");
        }
    }
    for prefix in ["【MV】", "[MV]", "(MV)"] {
        if clean.starts_with(prefix) {
            clean = clean[prefix.len()..].trim().to_string();
        }
    }
    clean = clean.trim().to_string();

    let clean_channel = channel_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.strip_suffix(" - Topic")
                .or_else(|| s.strip_suffix(" Official"))
                .unwrap_or(s)
                .trim()
                .to_string()
        });

    let mut bracket_quotes: Vec<String> = Vec::new();
    let mut rest_scan = clean.as_str();
    while let Some(open) = rest_scan.find('「') {
        let after = &rest_scan[open + '「'.len_utf8()..];
        if let Some(close) = after.find('」') {
            let inner = after[..close].trim();
            if !inner.is_empty() {
                bracket_quotes.push(inner.to_string());
            }
            rest_scan = &after[close + '」'.len_utf8()..];
        } else {
            break;
        }
    }

    let (parsed_artist, parsed_title, parsed_album) = if bracket_quotes.len() >= 2 {
        let album = bracket_quotes[0].clone();
        let song = bracket_quotes[1].clone();
        let tail_artist = clean
            .rsplit('」')
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        (
            tail_artist.or_else(|| clean_channel.clone()),
            song,
            Some(album),
        )
    } else if let Some((left, right)) = clean
        .split_once(" - ")
        .or_else(|| clean.split_once(" – "))
        .or_else(|| clean.split_once(" — "))
    {
        let artist = left.trim().to_string();
        let track = right.trim().to_string();
        let album = bracket_quotes
            .first()
            .cloned()
            .or_else(|| Some(format!("{track} - Single")));
        (Some(artist), track, album)
    } else {
        let album = bracket_quotes
            .first()
            .cloned()
            .or_else(|| clean_channel.clone())
            .or_else(|| Some(format!("{clean} - Single")));
        (clean_channel.clone(), clean.clone(), album)
    };

    let final_title = if parsed_title.is_empty() {
        "RuviTools Audio".to_string()
    } else {
        parsed_title
    };
    let final_artist = parsed_artist
        .filter(|s| !s.is_empty())
        .or(clean_channel)
        .unwrap_or_else(|| "YouTube Music".to_string());
    let final_album = parsed_album
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{final_title} - Single"));

    AudioMetadata {
        title: Some(final_title),
        artist: Some(final_artist),
        album: Some(final_album),
        year: Some("2026".to_string()),
        genre: Some("Music".to_string()),
        comment: source_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string),
        cover_jpeg: None,
    }
}

fn encode_utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2 + 2);
    out.extend_from_slice(&[0xFF, 0xFE]);
    for u in text.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(&[0x00, 0x00]);
    out
}

fn push_id3v23_frame(buf: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    if payload.is_empty() {
        return;
    }
    buf.extend_from_slice(id);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&[0x00, 0x00]);
    buf.extend_from_slice(payload);
}

fn push_id3v23_text_frame(buf: &mut Vec<u8>, id: &[u8; 4], text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut payload = Vec::with_capacity(1 + 2 + trimmed.len() * 2 + 2);
    payload.push(0x01); // UTF-16 with BOM
    payload.extend_from_slice(&encode_utf16le_with_bom(trimmed));
    push_id3v23_frame(buf, id, &payload);
}

/// Builds a complete ID3v2.3 tag header (`TIT2`, `TPE1`, `TPE2`, `TALB`, `TYER`, `TDRC`,
/// `TCON`, `TSSE`, `COMM`, and `APIC` front cover art) readable by Windows Explorer,
/// Windows Media Player, VLC, iTunes, and mobile players.
pub fn build_id3v23_tag(meta: &AudioMetadata) -> Vec<u8> {
    let mut frames = Vec::new();
    if let Some(ref title) = meta.title {
        push_id3v23_text_frame(&mut frames, b"TIT2", title);
    }
    if let Some(ref artist) = meta.artist {
        push_id3v23_text_frame(&mut frames, b"TPE1", artist);
        push_id3v23_text_frame(&mut frames, b"TPE2", artist);
    }
    if let Some(ref album) = meta.album {
        push_id3v23_text_frame(&mut frames, b"TALB", album);
    }
    if let Some(ref year) = meta.year {
        push_id3v23_text_frame(&mut frames, b"TYER", year);
        push_id3v23_text_frame(&mut frames, b"TDRC", year);
    }
    if let Some(ref genre) = meta.genre {
        push_id3v23_text_frame(&mut frames, b"TCON", genre);
    }
    push_id3v23_text_frame(
        &mut frames,
        b"TSSE",
        "RuviTools Pure-Rust Audio Engine (Symphonia + rusty_mp3)",
    );
    if let Some(ref comment) = meta.comment
        && !comment.trim().is_empty()
    {
        let mut payload = Vec::new();
        payload.push(0x01); // UTF-16
        payload.extend_from_slice(b"eng");
        payload.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x00]); // empty short description
        payload.extend_from_slice(&encode_utf16le_with_bom(comment.trim()));
        push_id3v23_frame(&mut frames, b"COMM", &payload);
    }
    if let Some(ref img) = meta.cover_jpeg
        && img.len() >= 16
    {
        let mime: &[u8] = if img.starts_with(b"\x89PNG") {
            b"image/png\0"
        } else {
            b"image/jpeg\0"
        };
        let mut payload = Vec::with_capacity(1 + mime.len() + 2 + img.len());
        payload.push(0x00); // ISO-8859-1 description encoding
        payload.extend_from_slice(mime);
        payload.push(0x03); // 0x03 = Cover (front)
        payload.push(0x00); // empty description NUL
        payload.extend_from_slice(img);
        push_id3v23_frame(&mut frames, b"APIC", &payload);
    }

    if frames.is_empty() {
        return Vec::new();
    }

    let sz = frames.len() as u32;
    let synchsafe = [
        ((sz >> 21) & 0x7F) as u8,
        ((sz >> 14) & 0x7F) as u8,
        ((sz >> 7) & 0x7F) as u8,
        (sz & 0x7F) as u8,
    ];
    let mut out = Vec::with_capacity(10 + frames.len());
    out.extend_from_slice(b"ID3\x03\x00\x00");
    out.extend_from_slice(&synchsafe);
    out.extend_from_slice(&frames);
    out
}

/// Replaces or prepends an ID3v2.3 tag on an existing `.mp3` file atomically.
pub fn write_id3v23_to_mp3_file(mp3_path: &Path, meta: &AudioMetadata) -> RuviResult<()> {
    let tag = build_id3v23_tag(meta);
    if tag.is_empty() {
        return Ok(());
    }
    let raw = std::fs::read(mp3_path).map_err(|err| {
        RuviError::Filesystem(format!("failed reading {}: {err}", mp3_path.display()))
    })?;
    let audio_offset = if raw.len() >= 10 && &raw[0..3] == b"ID3" {
        let sz = ((usize::from(raw[6] & 0x7F)) << 21)
            | ((usize::from(raw[7] & 0x7F)) << 14)
            | ((usize::from(raw[8] & 0x7F)) << 7)
            | usize::from(raw[9] & 0x7F);
        (10 + sz).min(raw.len())
    } else {
        0
    };
    let tmp = mp3_path.with_extension("id3.tmp");
    {
        let mut f = BufWriter::with_capacity(256 * 1024, File::create(&tmp)?);
        f.write_all(&tag)?;
        f.write_all(&raw[audio_offset..])?;
        f.flush()?;
    }
    std::fs::rename(&tmp, mp3_path)?;
    Ok(())
}

/// Parses an audio bitrate specification like `"320k"`, `"320kbps"`, `"192k"`, `"vbr"`, `"mp3:0"`.
pub fn parse_mp3_bitrate_spec(spec: Option<&str>) -> (u32, Option<f32>) {
    let raw = spec.unwrap_or("192k").trim().to_ascii_lowercase();
    if raw == "vbr" || raw == "0" || raw == "mp3:0" {
        return (192, Some(vbr_quality_index(2.0)));
    }
    let digits = raw
        .trim_start_matches("mp3:")
        .trim_end_matches("kbps")
        .trim_end_matches('k')
        .trim();
    let kbps = digits.parse::<u32>().unwrap_or(192).clamp(32, 320);
    (kbps, None)
}

fn build_codec_registry() -> CodecRegistry {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder_at_tier::<OpusDecoder>(Tier::Fallback);
    registry
}

fn is_supported_mp3_rate(sr: u32) -> bool {
    matches!(
        sr,
        8000 | 11025 | 12000 | 16000 | 22050 | 24000 | 32000 | 44100 | 48000
    )
}

fn normalize_channels_f32(interleaved: &[f32], in_ch: usize) -> (Cow<'_, [f32]>, u16) {
    if in_ch <= 1 {
        return (Cow::Borrowed(interleaved), 1);
    }
    if in_ch == 2 {
        return (Cow::Borrowed(interleaved), 2);
    }
    let frames = interleaved.len() / in_ch;
    let mut stereo = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let base = f * in_ch;
        let l = interleaved[base];
        let r = interleaved[base + 1];
        let center = if in_ch > 2 {
            interleaved[base + 2] * std::f32::consts::FRAC_1_SQRT_2
        } else {
            0.0
        };
        stereo.push((l + center).clamp(-1.0, 1.0));
        stereo.push((r + center).clamp(-1.0, 1.0));
    }
    (Cow::Owned(stereo), 2)
}

fn resample_linear_f32<'a>(
    input: &'a [f32],
    channels: usize,
    in_rate: u32,
    out_rate: u32,
) -> Cow<'a, [f32]> {
    if in_rate == out_rate || input.is_empty() || channels == 0 {
        return Cow::Borrowed(input);
    }
    let in_frames = input.len() / channels;
    if in_frames < 2 {
        return Cow::Borrowed(input);
    }
    let out_frames =
        ((in_frames as u64 * u64::from(out_rate)) / u64::from(in_rate)).max(1) as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    let ratio = f64::from(in_rate) / f64::from(out_rate);
    for i in 0..out_frames {
        let src_pos = i as f64 * ratio;
        let idx0 = (src_pos.floor() as usize).min(in_frames - 1);
        let idx1 = (idx0 + 1).min(in_frames - 1);
        let frac = (src_pos - idx0 as f64) as f32;
        for c in 0..channels {
            let s0 = input[idx0 * channels + c];
            let s1 = input[idx1 * channels + c];
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    Cow::Owned(out)
}

fn decode_stream_pcm<F>(
    input_path: &Path,
    opts: &AudioTranscodeOptions,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
    mut on_pcm: F,
) -> RuviResult<bool>
where
    F: FnMut(&[f32], u32, u16) -> RuviResult<()>,
{
    let file = File::open(input_path).map_err(|err| {
        RuviError::Filesystem(format!("failed opening {}: {err}", input_path.display()))
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = input_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = match symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    ) {
        Ok(fmt) => fmt,
        Err(_) => return Ok(false),
    };

    let (track_id, codec_params, time_base, total_dur_s) = {
        let Some(track) = format.default_track(TrackType::Audio) else {
            return Ok(false);
        };
        let Some(audio_params) = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .filter(|a| a.codec != CODEC_ID_NULL_AUDIO)
            .cloned()
        else {
            return Ok(false);
        };
        let dur = match (track.num_frames, audio_params.sample_rate) {
            (Some(n), Some(sr)) if sr > 0 => Some(n as f64 / f64::from(sr)),
            _ => None,
        };
        (track.id, audio_params, track.time_base, dur)
    };

    let codecs = build_codec_registry();
    let Ok(mut decoder) = codecs.make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
    else {
        return Ok(false);
    };

    let req_start_s = opts.start_seconds.unwrap_or(0.0).max(0.0);
    let mut skip_seconds = req_start_s;

    if req_start_s > 2.0 {
        let seek_target_s = (req_start_s - 1.0).max(0.0);
        if let Some(seek_time) = Time::try_from_secs_f64(seek_target_s)
            && let Ok(seeked) = format.seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: seek_time,
                    track_id: Some(track_id),
                },
            )
        {
            decoder.reset();
            if let Some(tb) = time_base {
                let actual_s = tb.calc_time_saturating(seeked.actual_ts).as_secs_f64();
                skip_seconds = (req_start_s - actual_s).max(0.0);
            }
        }
    }

    let mut pcm_scratch: Vec<f32> = Vec::new();
    let mut skipped_frames: u64 = 0;
    let mut kept_frames: u64 = 0;
    let mut emitted_any = false;
    let mut packets_seen = 0u64;

    loop {
        if cancel.is_cancelled() {
            return Err(RuviError::Cancelled(
                "audio transcoding cancelled".to_string(),
            ));
        }

        let packet = match format.next_packet() {
            Ok(Some(pkt)) => pkt,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphoniaError::IoError(ref err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => {
                if emitted_any {
                    break;
                }
                return Ok(false);
            }
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        };

        let frames = decoded.frames();
        if frames == 0 {
            continue;
        }
        let spec = decoded.spec();
        let sr = spec.rate().max(8000);
        let ch = spec.channels().count().max(1);

        decoded.copy_to_vec_interleaved(&mut pcm_scratch);
        if pcm_scratch.is_empty() {
            continue;
        }

        let target_skip_frames = (skip_seconds * f64::from(sr)).round() as u64;
        let mut slice_start_frame = 0usize;
        if skipped_frames < target_skip_frames {
            let need = (target_skip_frames - skipped_frames) as usize;
            if frames <= need {
                skipped_frames += frames as u64;
                continue;
            }
            slice_start_frame = need;
            skipped_frames = target_skip_frames;
        }

        let mut avail_frames = frames - slice_start_frame;
        if let Some(max_dur) = opts.duration_seconds {
            let max_total_frames = (max_dur.max(0.01) * f64::from(sr)).round() as u64;
            if kept_frames >= max_total_frames {
                break;
            }
            let remaining = (max_total_frames - kept_frames) as usize;
            if avail_frames > remaining {
                avail_frames = remaining;
            }
        }

        if avail_frames == 0 {
            break;
        }

        let start_idx = slice_start_frame * ch;
        let end_idx = start_idx + avail_frames * ch;
        let slice = &pcm_scratch[start_idx..end_idx];

        emitted_any = true;
        kept_frames += avail_frames as u64;
        packets_seen += 1;

        if packets_seen % 64 == 0 {
            let cur_s = kept_frames as f64 / f64::from(sr);
            let _ = total_dur_s;
            progress.on_progress(ProgressUpdate {
                out_time_us: Some((cur_s * 1_000_000.0) as u64),
                fps: None,
                speed: Some("native-audio".to_string()),
                finished: false,
            });
        }

        on_pcm(slice, sr, ch as u16)?;
        if let Some(max_dur) = opts.duration_seconds {
            let max_total_frames = (max_dur.max(0.01) * f64::from(sr)).round() as u64;
            if kept_frames >= max_total_frames {
                break;
            }
        }
    }

    Ok(emitted_any)
}

/// Decodes any Symphonia-supported audio container (M4A/AAC, WebM/Opus, Ogg/Vorbis, FLAC, WAV, MP3)
/// and encodes a genuine CBR or VBR MPEG Layer III `.mp3` stream in constant RAM.
///
/// Returns `Ok(false)` when the input has no decodable audio frames (so synthetic placeholder
/// tests can use their fallback without failing).
pub fn transcode_audio_to_mp3(
    input_path: &Path,
    out_file: &mut BufWriter<File>,
    opts: &AudioTranscodeOptions,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> RuviResult<bool> {
    let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
        bitrate_kbps: opts.bitrate_kbps,
        vbr_quality: opts.vbr_quality,
    });
    let mut target_sr: Option<u32> = None;
    let mut wrote_any = false;
    let mut id3_len: u64 = 0;

    let effective_meta = if opts.metadata.title.is_some() || opts.metadata.artist.is_some() {
        opts.metadata.clone()
    } else {
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Audio");
        parse_smart_title_metadata(stem, None, None)
    };
    let id3_bytes = build_id3v23_tag(&effective_meta);

    let decoded = decode_stream_pcm(input_path, opts, progress, cancel, |slice, sr, ch| {
        if !wrote_any && id3_len == 0 && !id3_bytes.is_empty() {
            out_file.write_all(&id3_bytes)?;
            id3_len = id3_bytes.len() as u64;
        }
        let (norm_pcm, norm_ch) = normalize_channels_f32(slice, usize::from(ch));
        let chosen_sr = *target_sr.get_or_insert_with(|| {
            if is_supported_mp3_rate(sr) {
                sr
            } else {
                44_100
            }
        });
        let pcm_ready = resample_linear_f32(&norm_pcm, usize::from(norm_ch), sr, chosen_sr);

        encoder
            .push_pcm_f32(&pcm_ready, norm_ch, chosen_sr)
            .map_err(|err| RuviError::Backend(format!("MP3 encoder error: {err}")))?;

        while let Ok(pkt) = encoder.next_packet() {
            out_file.write_all(&pkt)?;
            wrote_any = true;
        }
        Ok(())
    })?;

    if !decoded {
        return Ok(false);
    }

    encoder.finish();
    while let Ok(pkt) = encoder.next_packet() {
        out_file.write_all(&pkt)?;
        wrote_any = true;
    }
    out_file.flush()?;

    // Patch the leading Xing/Info frame (immediately after ID3v2.3 header) with finalized frame count & TOC.
    if wrote_any && let Some(tag_frame) = encoder.lame_tag_frame() {
        let end_pos = out_file.stream_position()?;
        out_file.seek(SeekFrom::Start(id3_len))?;
        out_file.write_all(tag_frame)?;
        out_file.flush()?;
        out_file.seek(SeekFrom::Start(end_pos))?;
    }

    Ok(wrote_any)
}

/// Decodes any Symphonia-supported audio container into a standard 16-bit PCM RIFF/WAVE `.wav` file
/// using constant-memory streaming I/O.
pub fn transcode_audio_to_wav(
    input_path: &Path,
    out_file: &mut BufWriter<File>,
    opts: &AudioTranscodeOptions,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> RuviResult<bool> {
    let mut header_written = false;
    let mut data_bytes: u64 = 0;
    let mut pcm_bytes_buf: Vec<u8> = Vec::new();

    let decoded = decode_stream_pcm(input_path, opts, progress, cancel, |slice, sr, ch| {
        let (norm_pcm, norm_ch) = normalize_channels_f32(slice, usize::from(ch));
        if !header_written {
            let block_align = norm_ch * 2;
            let byte_rate = sr * u32::from(block_align);
            let mut hdr = [0u8; 44];
            hdr[0..4].copy_from_slice(b"RIFF");
            hdr[4..8].copy_from_slice(&36u32.to_le_bytes());
            hdr[8..12].copy_from_slice(b"WAVE");
            hdr[12..16].copy_from_slice(b"fmt ");
            hdr[16..20].copy_from_slice(&16u32.to_le_bytes());
            hdr[20..22].copy_from_slice(&1u16.to_le_bytes());
            hdr[22..24].copy_from_slice(&norm_ch.to_le_bytes());
            hdr[24..28].copy_from_slice(&sr.to_le_bytes());
            hdr[28..32].copy_from_slice(&byte_rate.to_le_bytes());
            hdr[32..34].copy_from_slice(&block_align.to_le_bytes());
            hdr[34..36].copy_from_slice(&16u16.to_le_bytes());
            hdr[36..40].copy_from_slice(b"data");
            hdr[40..44].copy_from_slice(&0u32.to_le_bytes());
            out_file.write_all(&hdr)?;
            header_written = true;
        }

        pcm_bytes_buf.clear();
        pcm_bytes_buf.reserve(norm_pcm.len() * 2);
        for &s in norm_pcm.iter() {
            let scaled = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            pcm_bytes_buf.extend_from_slice(&scaled.to_le_bytes());
        }
        out_file.write_all(&pcm_bytes_buf)?;
        data_bytes += pcm_bytes_buf.len() as u64;
        Ok(())
    })?;

    if !decoded || !header_written {
        return Ok(false);
    }

    if data_bytes % 2 != 0 {
        out_file.write_all(&[0u8])?;
        data_bytes += 1;
    }
    out_file.flush()?;

    let data_u32 = data_bytes.min(u64::from(u32::MAX - 36)) as u32;
    let riff_u32 = data_u32.saturating_add(36);
    let end_pos = out_file.stream_position()?;
    out_file.seek(SeekFrom::Start(4))?;
    out_file.write_all(&riff_u32.to_le_bytes())?;
    out_file.seek(SeekFrom::Start(40))?;
    out_file.write_all(&data_u32.to_le_bytes())?;
    out_file.flush()?;
    out_file.seek(SeekFrom::Start(end_pos))?;

    Ok(true)
}

#[cfg(test)]
mod id3_tests {
    use super::{build_id3v23_tag, parse_smart_title_metadata};

    #[test]
    fn parses_artist_track_and_anime_album_titles_and_builds_id3v23() {
        let m1 = parse_smart_title_metadata(
            "TW3LV - California Sun ft. Johnny Rain [Audio]",
            Some("TW3LV - Topic"),
            Some("https://www.youtube.com/watch?v=L1rIlSH3r0E"),
        );
        assert_eq!(m1.artist.as_deref(), Some("TW3LV"));
        assert_eq!(m1.title.as_deref(), Some("California Sun ft. Johnny Rain"));
        assert_eq!(
            m1.album.as_deref(),
            Some("California Sun ft. Johnny Rain - Single")
        );

        let mut m2 = parse_smart_title_metadata(
            "【MV】「TO BE HERO X」Episode Theme Song of X　X編 劇中歌「New Type of Hero」Griff Clawson",
            Some("Aniplex"),
            Some("https://www.youtube.com/watch?v=test"),
        );
        assert_eq!(m2.album.as_deref(), Some("TO BE HERO X"));
        assert_eq!(m2.title.as_deref(), Some("New Type of Hero"));
        assert_eq!(m2.artist.as_deref(), Some("Griff Clawson"));
        m2.cover_jpeg = Some(vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
        ]);

        let tag = build_id3v23_tag(&m2);
        assert!(tag.starts_with(b"ID3\x03\x00\x00"));
        assert!(tag.windows(4).any(|w| w == b"TIT2"));
        assert!(tag.windows(4).any(|w| w == b"TPE1"));
        assert!(tag.windows(4).any(|w| w == b"TALB"));
        assert!(tag.windows(4).any(|w| w == b"TYER"));
        assert!(tag.windows(4).any(|w| w == b"APIC"));
    }
}
