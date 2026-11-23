mod common;

use std::path::Path;

use predicates::prelude::*;
use ruvic::backend::native::ebml::{EbmlBuildSpec, build_ebml_bytes};
use ruvic::backend::native::mp4_atoms::{Mp4BuildSpec, build_mp4_bytes};
use ruvic::domain::media_info::MediaInfo;
use ruvic::domain::stream::{AudioCodec, ContainerFormat, StreamKind, VideoCodec};
use ruvic::operations::batch::BatchReport;

fn native_cmd(isolated_dir: &Path) -> assert_cmd::Command {
    let empty_path_dir = isolated_dir.join("empty_path");
    std::fs::create_dir_all(&empty_path_dir).unwrap();
    let mut cmd = common::ruvic_cmd(isolated_dir);
    cmd.env("PATH", &empty_path_dir)
        .env_remove("RUVIC_FFMPEG_PATH")
        .env_remove("RUVIC_FFPROBE_PATH");
    cmd
}

fn assert_no_temp_files(dir: &Path) {
    let leftovers: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".ruvic-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "expected zero .ruvic-tmp-* files in {}, found: {:?}",
        dir.display(),
        leftovers
    );
}

fn build_sample_mp4_1080p(duration_seconds: f64) -> Vec<u8> {
    build_mp4_bytes(&Mp4BuildSpec {
        container: ContainerFormat::Mp4,
        duration_seconds,
        video: Some((VideoCodec::H264, 1920, 1080)),
        audio: Some((AudioCodec::Aac, 48_000, 2)),
        mdat_payload: b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_NATIVE_MP4_PAYLOAD",
        video_aux: None,
    })
}

fn probe_json(isolated_dir: &Path, media_path: &Path) -> MediaInfo {
    let output = native_cmd(isolated_dir)
        .args(["probe", media_path.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("valid MediaInfo JSON from ruvic probe")
}

fn find_mp4_box_offset(data: &[u8], tag: &[u8; 4]) -> Option<usize> {
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
        if size < 8 || pos + size > data.len() {
            return None;
        }
        if &data[pos + 4..pos + 8] == tag {
            return Some(pos);
        }
        pos += size;
    }
    None
}

#[test]
fn native_probe_parses_1080p_mp4_and_faststart_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("sample_1080p.mp4");
    let bytes = build_sample_mp4_1080p(15.0);

    // Verify faststart box layout: ftyp at 0, moov before mdat
    assert_eq!(find_mp4_box_offset(&bytes, b"ftyp"), Some(0));
    let moov_off = find_mp4_box_offset(&bytes, b"moov").expect("moov atom");
    let mdat_off = find_mp4_box_offset(&bytes, b"mdat").expect("mdat atom");
    assert!(moov_off < mdat_off, "expected faststart moov before mdat");

    std::fs::write(&input, &bytes).unwrap();

    let info = probe_json(tmp.path(), &input);
    assert_eq!(info.container.format_name, "mov,mp4,m4a,3gp,3g2,mj2");
    let dur = info.container.duration_seconds.expect("duration_seconds");
    assert!((dur - 15.0).abs() < 0.01, "expected ~15.0s, got {dur}");
    assert_eq!(info.streams.len(), 2);

    let video = info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Video)
        .expect("video stream");
    assert_eq!(video.codec_name, "h264");
    assert_eq!(video.width, Some(1920));
    assert_eq!(video.height, Some(1080));

    let audio = info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .expect("audio stream");
    assert_eq!(audio.codec_name, "aac");
    assert_eq!(audio.sample_rate, Some(48_000));
    assert_eq!(audio.channels, Some(2));
}

#[test]
fn native_convert_presets_web_mp4_archive_mkv_and_fast_720p() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("source.mp4");
    std::fs::write(&input, build_sample_mp4_1080p(15.0)).unwrap();

    // 1. web-mp4
    let web_mp4 = tmp.path().join("web.mp4");
    native_cmd(tmp.path())
        .args([
            "convert",
            input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            web_mp4.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains(
            r#""selected_video_encoder": "libx264""#,
        ));

    let web_bytes = std::fs::read(&web_mp4).unwrap();
    let moov_off = find_mp4_box_offset(&web_bytes, b"moov").expect("web.mp4 moov");
    let mdat_off = find_mp4_box_offset(&web_bytes, b"mdat").expect("web.mp4 mdat");
    assert!(moov_off < mdat_off, "web-mp4 output must be faststart");

    let web_info = probe_json(tmp.path(), &web_mp4);
    assert_eq!(web_info.container.format_name, "mov,mp4,m4a,3gp,3g2,mj2");
    assert!((web_info.container.duration_seconds.unwrap() - 15.0).abs() < 0.01);
    let web_v = web_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Video)
        .unwrap();
    assert_eq!(web_v.codec_name, "h264");
    assert_eq!((web_v.width, web_v.height), (Some(1920), Some(1080)));
    let web_a = web_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .unwrap();
    assert_eq!(web_a.codec_name, "aac");
    assert_eq!(web_a.sample_rate, Some(48_000));

    // 2. archive-mkv
    let archive_mkv = tmp.path().join("archive.mkv");
    native_cmd(tmp.path())
        .args([
            "convert",
            input.to_str().unwrap(),
            "--preset",
            "archive-mkv",
            "-o",
            archive_mkv.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""selected_video_encoder": "libx265""#,
        ))
        .stdout(predicate::str::contains(
            r#""selected_audio_encoder": "libopus""#,
        ));

    let mkv_bytes = std::fs::read(&archive_mkv).unwrap();
    assert_eq!(&mkv_bytes[0..4], &[0x1A, 0x45, 0xDF, 0xA3]);
    let mkv_info = probe_json(tmp.path(), &archive_mkv);
    assert_eq!(mkv_info.container.format_name, "matroska");
    assert!((mkv_info.container.duration_seconds.unwrap() - 15.0).abs() < 0.01);
    let mkv_v = mkv_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Video)
        .unwrap();
    assert_eq!(mkv_v.codec_name, "hevc");
    assert_eq!((mkv_v.width, mkv_v.height), (Some(1920), Some(1080)));
    let mkv_a = mkv_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .unwrap();
    assert_eq!(mkv_a.codec_name, "opus");

    // 3. fast-720p (verifying max_height = 720 downscaling)
    let fast_720 = tmp.path().join("fast_720p.mp4");
    native_cmd(tmp.path())
        .args([
            "convert",
            input.to_str().unwrap(),
            "--preset",
            "fast-720p",
            "-o",
            fast_720.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#));

    let scaled_info = probe_json(tmp.path(), &fast_720);
    let scaled_v = scaled_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Video)
        .unwrap();
    assert_eq!(scaled_v.codec_name, "h264");
    assert_eq!(scaled_v.width, Some(1280));
    assert_eq!(scaled_v.height, Some(720));
    assert_no_temp_files(tmp.path());
}

#[test]
fn native_remux_mp4_to_mkv_and_rejects_vp9_webm_to_mp4() {
    let tmp = tempfile::tempdir().unwrap();
    let input_mp4 = tmp.path().join("source.mp4");
    std::fs::write(&input_mp4, build_sample_mp4_1080p(15.0)).unwrap();

    let out_mkv = tmp.path().join("remuxed.mkv");
    native_cmd(tmp.path())
        .args([
            "remux",
            input_mp4.to_str().unwrap(),
            "-o",
            out_mkv.to_str().unwrap(),
            "--copy",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""operation": "remux""#));

    let mkv_info = probe_json(tmp.path(), &out_mkv);
    assert_eq!(mkv_info.container.format_name, "matroska");
    let v = mkv_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Video)
        .unwrap();
    assert_eq!(v.codec_name, "h264");
    assert_eq!((v.width, v.height), (Some(1920), Some(1080)));
    let a = mkv_info
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .unwrap();
    assert_eq!(a.codec_name, "aac");

    // WebM VP9 + Opus -> MP4 remux must be rejected with exit code 4 (Unsupported)
    let input_webm = tmp.path().join("vp9_clip.webm");
    let webm_bytes = build_ebml_bytes(&EbmlBuildSpec {
        container: ContainerFormat::Webm,
        duration_seconds: 8.0,
        video: Some((VideoCodec::Vp9, 1280, 720)),
        audio: Some((AudioCodec::Opus, 48_000, 2)),
        payload: b"VP9_OPUS_NATIVE_PAYLOAD",
        video_colour: None,
        video_projection: None,
    });
    std::fs::write(&input_webm, webm_bytes).unwrap();

    let webm_info = probe_json(tmp.path(), &input_webm);
    assert_eq!(webm_info.container.format_name, "matroska,webm");
    assert_eq!(webm_info.streams[0].codec_name, "vp9");

    let bad_mp4 = tmp.path().join("rejected.mp4");
    native_cmd(tmp.path())
        .args([
            "remux",
            input_webm.to_str().unwrap(),
            "-o",
            bad_mp4.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .code(4)
        .stdout(predicate::str::contains(r#""category": "unsupported""#));
    assert!(!bad_mp4.exists());
    assert_no_temp_files(tmp.path());
}

#[test]
fn native_audio_extracts_mp3_flac_wav_ogg_and_aac_and_all_probe() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("source.mp4");
    std::fs::write(&input, build_sample_mp4_1080p(15.0)).unwrap();

    let cases = [
        ("mp3", "out.mp3", "mp3", "mp3"),
        ("flac", "out.flac", "flac", "flac"),
        ("wav", "out.wav", "wav", "pcm_s16le"),
        ("ogg", "out.ogg", "ogg", "opus"),
        ("aac", "out.aac", "aac", "aac"),
    ];

    for (to_fmt, filename, expected_fmt, expected_codec) in cases {
        let out_path = tmp.path().join(filename);
        native_cmd(tmp.path())
            .args([
                "audio",
                input.to_str().unwrap(),
                "--to",
                to_fmt,
                "-o",
                out_path.to_str().unwrap(),
                "--json",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(r#""operation": "extract_audio""#));

        let info = probe_json(tmp.path(), &out_path);
        assert_eq!(info.container.format_name, expected_fmt);
        assert_eq!(info.streams.len(), 1);
        assert_eq!(info.streams[0].kind, StreamKind::Audio);
        assert_eq!(info.streams[0].codec_name, expected_codec);
        assert_eq!(info.streams[0].sample_rate, Some(48_000));
        assert_eq!(info.streams[0].channels, Some(2));
    }

    // Stream copy AAC from MP4 -> ADTS AAC succeeds
    let copied_aac = tmp.path().join("copied.aac");
    native_cmd(tmp.path())
        .args([
            "audio",
            input.to_str().unwrap(),
            "--copy",
            "-o",
            copied_aac.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();
    let copied_info = probe_json(tmp.path(), &copied_aac);
    assert_eq!(copied_info.streams[0].codec_name, "aac");

    // Stream copy AAC -> MP3 rejected with exit code 4
    let bad_copy_mp3 = tmp.path().join("bad_copy.mp3");
    native_cmd(tmp.path())
        .args([
            "audio",
            input.to_str().unwrap(),
            "--to",
            "mp3",
            "--copy",
            "-o",
            bad_copy_mp3.to_str().unwrap(),
        ])
        .assert()
        .code(4);
    assert!(!bad_copy_mp3.exists());
    assert_no_temp_files(tmp.path());
}

#[test]
fn native_trim_slices_duration_accurately() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("source.mp4");
    std::fs::write(&input, build_sample_mp4_1080p(15.0)).unwrap();

    let trimmed = tmp.path().join("trimmed_5s.mp4");
    native_cmd(tmp.path())
        .args([
            "trim",
            input.to_str().unwrap(),
            "--start",
            "00:00:02",
            "--duration",
            "5",
            "-o",
            trimmed.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""operation": "trim""#));

    let info = probe_json(tmp.path(), &trimmed);
    let dur = info.container.duration_seconds.expect("trimmed duration");
    assert!(
        (dur - 5.0).abs() <= 0.1,
        "expected trimmed duration ~5.0s, got {dur}"
    );
    assert_eq!(info.streams.len(), 2);

    // Also verify --end range trimming
    let trimmed_end = tmp.path().join("trimmed_end.mp4");
    native_cmd(tmp.path())
        .args([
            "trim",
            input.to_str().unwrap(),
            "--start",
            "00:00:03",
            "--end",
            "00:00:09",
            "-o",
            trimmed_end.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    let end_info = probe_json(tmp.path(), &trimmed_end);
    let end_dur = end_info.container.duration_seconds.expect("end duration");
    assert!(
        (end_dur - 6.0).abs() <= 0.1,
        "expected trimmed duration ~6.0s, got {end_dur}"
    );
    assert_no_temp_files(tmp.path());
}

#[test]
fn native_thumbnail_writes_valid_jpg_png_and_webp_signatures() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("source.mp4");
    std::fs::write(&input, build_sample_mp4_1080p(15.0)).unwrap();

    // 1. JPEG
    let jpg = tmp.path().join("thumb.jpg");
    native_cmd(tmp.path())
        .args([
            "thumbnail",
            input.to_str().unwrap(),
            "--at",
            "00:00:05",
            "-o",
            jpg.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""operation": "thumbnail""#));
    let jpg_bytes = std::fs::read(&jpg).unwrap();
    assert!(jpg_bytes.starts_with(&[0xFF, 0xD8, 0xFF]));
    assert!(jpg_bytes.ends_with(&[0xFF, 0xD9]));
    let jpg_info = probe_json(tmp.path(), &jpg);
    assert_eq!(jpg_info.container.format_name, "jpeg_pipe");
    assert_eq!(
        (jpg_info.streams[0].width, jpg_info.streams[0].height),
        (Some(1920), Some(1080))
    );

    // 2. PNG
    let png = tmp.path().join("thumb.png");
    native_cmd(tmp.path())
        .args([
            "thumbnail",
            input.to_str().unwrap(),
            "--at",
            "00:00:05",
            "-o",
            png.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();
    let png_bytes = std::fs::read(&png).unwrap();
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1A\n"));
    assert_eq!(&png_bytes[12..16], b"IHDR");
    assert!(png_bytes.windows(4).any(|w| w == b"IEND"));
    let png_info = probe_json(tmp.path(), &png);
    assert_eq!(png_info.container.format_name, "png_pipe");
    assert_eq!(
        (png_info.streams[0].width, png_info.streams[0].height),
        (Some(1920), Some(1080))
    );

    // 3. WebP
    let webp = tmp.path().join("thumb.webp");
    native_cmd(tmp.path())
        .args([
            "thumbnail",
            input.to_str().unwrap(),
            "--at",
            "00:00:05",
            "-o",
            webp.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();
    let webp_bytes = std::fs::read(&webp).unwrap();
    assert!(webp_bytes.starts_with(b"RIFF"));
    assert_eq!(&webp_bytes[8..12], b"WEBP");
    assert_eq!(&webp_bytes[12..16], b"VP8 ");
    let webp_info = probe_json(tmp.path(), &webp);
    assert_eq!(webp_info.container.format_name, "webp");
    assert_eq!(
        (webp_info.streams[0].width, webp_info.streams[0].height),
        (Some(1920), Some(1080))
    );

    assert_no_temp_files(tmp.path());
}

#[test]
fn native_batch_converts_directory_of_pure_rust_mp4_files() {
    let tmp = tempfile::tempdir().unwrap();
    let batch_in = tmp.path().join("in_dir");
    let batch_out = tmp.path().join("out_dir");
    std::fs::create_dir_all(&batch_in).unwrap();

    for name in ["alpha.mp4", "beta.mp4", "gamma.mp4"] {
        std::fs::write(batch_in.join(name), build_sample_mp4_1080p(10.0)).unwrap();
    }
    // Non-media file must be ignored by batch scanner
    std::fs::write(batch_in.join("readme.txt"), b"ignore me").unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "batch",
            batch_in.to_str().unwrap(),
            "-o",
            batch_out.to_str().unwrap(),
            "--preset",
            "fast-720p",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let report: BatchReport = serde_json::from_slice(&output).expect("valid BatchReport JSON");
    assert!(report.ok);
    assert_eq!(report.total, 3);
    assert_eq!(report.succeeded, 3);
    assert_eq!(report.failed, 0);

    for name in ["alpha.mp4", "beta.mp4", "gamma.mp4"] {
        let out_file = batch_out.join(name);
        assert!(out_file.is_file(), "missing batch output {name}");
        let info = probe_json(tmp.path(), &out_file);
        assert_eq!(info.streams[0].width, Some(1280));
        assert_eq!(info.streams[0].height, Some(720));
    }
    assert_no_temp_files(&batch_out);
}

#[test]
fn native_corrupt_and_invalid_inputs_fail_with_integrity_code_8_and_clean_temp() {
    let tmp = tempfile::tempdir().unwrap();

    let corrupt_random = tmp.path().join("garbage.mp4");
    std::fs::write(&corrupt_random, b"NOT_A_VALID_MEDIA_CONTAINER_BYTES").unwrap();

    let valid_mp4 = build_sample_mp4_1080p(10.0);
    let truncated_mp4 = tmp.path().join("truncated.mp4");
    std::fs::write(&truncated_mp4, &valid_mp4[..valid_mp4.len() / 2]).unwrap();

    let truncated_ebml = tmp.path().join("truncated.mkv");
    std::fs::write(&truncated_ebml, [0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86]).unwrap();

    let empty_mp4 = tmp.path().join("empty.mp4");
    std::fs::write(&empty_mp4, b"").unwrap();

    for bad_input in [&corrupt_random, &truncated_mp4, &truncated_ebml, &empty_mp4] {
        native_cmd(tmp.path())
            .args(["probe", bad_input.to_str().unwrap(), "--json"])
            .assert()
            .code(8)
            .stdout(predicate::str::contains(r#""category": "integrity""#));

        let out_mp4 = tmp.path().join("should_not_exist.mp4");
        native_cmd(tmp.path())
            .args([
                "convert",
                bad_input.to_str().unwrap(),
                "--preset",
                "web-mp4",
                "-o",
                out_mp4.to_str().unwrap(),
                "--json",
            ])
            .assert()
            .code(8)
            .stdout(predicate::str::contains(r#""category": "integrity""#));
        assert!(!out_mp4.exists());
        assert_no_temp_files(tmp.path());
    }

    // Same input/output path and overwrite protection in pure-Rust mode
    let valid_file = tmp.path().join("valid.mp4");
    std::fs::write(&valid_file, &valid_mp4).unwrap();
    native_cmd(tmp.path())
        .args([
            "convert",
            valid_file.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            valid_file.to_str().unwrap(),
            "--overwrite",
        ])
        .assert()
        .code(2);
    assert_eq!(std::fs::read(&valid_file).unwrap(), valid_mp4);

    let protected = tmp.path().join("existing.mp4");
    let sentinel = b"KEEP_EXISTING_FILE";
    std::fs::write(&protected, sentinel).unwrap();
    native_cmd(tmp.path())
        .args([
            "remux",
            valid_file.to_str().unwrap(),
            "-o",
            protected.to_str().unwrap(),
        ])
        .assert()
        .code(5);
    assert_eq!(std::fs::read(&protected).unwrap(), sentinel);
    assert_no_temp_files(tmp.path());
}

fn build_pcm_wav_sine(sample_rate: u32, channels: u16, duration_s: f64, freq_hz: f64) -> Vec<u8> {
    let total_frames = (f64::from(sample_rate) * duration_s).round() as usize;
    let data_bytes = total_frames * usize::from(channels) * 2;
    let block_align = channels * 2;
    let byte_rate = sample_rate * u32::from(block_align);

    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());

    for i in 0..total_frames {
        let t = i as f64 / f64::from(sample_rate);
        let val = (2.0 * std::f64::consts::PI * freq_hz * t).sin() * 0.4;
        let sample = (val * 32767.0).round() as i16;
        for _ in 0..channels {
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    out
}

fn decode_mp3_stats(mp3_bytes: &[u8]) -> (u32, u16, usize, f64) {
    let mut dec = rusty_mp3::Mp3Decoder::new();
    dec.push(mp3_bytes);
    dec.flush();
    let mut sr = 0u32;
    let mut ch = 0u16;
    let mut samples = Vec::new();
    while let Ok(frame) = dec.next_frame() {
        sr = frame.sample_rate;
        ch = frame.channels;
        samples.extend_from_slice(&frame.samples);
    }
    let rms = if samples.is_empty() {
        0.0
    } else {
        let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        (sum_sq / samples.len() as f64).sqrt()
    };
    (sr, ch, samples.len(), rms)
}

#[test]
fn native_audio_real_wav_to_mp3_cbr_and_vbr_decodes_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let wav_in = tmp.path().join("tone_2s.wav");
    std::fs::write(&wav_in, build_pcm_wav_sine(44_100, 2, 2.0, 440.0)).unwrap();

    for (bitrate, fname) in [
        ("320k", "out_320.mp3"),
        ("192k", "out_192.mp3"),
        ("vbr", "out_vbr.mp3"),
    ] {
        let mp3_out = tmp.path().join(fname);
        native_cmd(tmp.path())
            .args([
                "audio",
                wav_in.to_str().unwrap(),
                "--to",
                "mp3",
                "--bitrate",
                bitrate,
                "-o",
                mp3_out.to_str().unwrap(),
                "--json",
            ])
            .assert()
            .success();

        let mp3_bytes = std::fs::read(&mp3_out).unwrap();
        assert!(
            mp3_bytes.len() > 8_000,
            "expected real MP3 audio for {bitrate}, got {} bytes",
            mp3_bytes.len()
        );

        let info = probe_json(tmp.path(), &mp3_out);
        assert_eq!(info.container.format_name, "mp3");
        assert_eq!(info.streams[0].sample_rate, Some(44_100));
        assert_eq!(info.streams[0].channels, Some(2));

        let (sr, ch, sample_count, rms) = decode_mp3_stats(&mp3_bytes);
        assert_eq!(sr, 44_100);
        assert_eq!(ch, 2);
        assert!(
            sample_count >= 44_100 * 2,
            "expected ~2s stereo PCM samples, got {sample_count}"
        );
        assert!(
            rms > 0.05,
            "decoded MP3 should contain 440 Hz tone, rms={rms}"
        );
    }

    // Verify sample-accurate audio slicing + WAV output
    let backend = ruvic::backend::native::NativeMediaBackend::new();
    let cfg = ruvic::config::RuvicConfig::default();
    let sliced_mp3 = tmp.path().join("sliced_1s.mp3");
    let sliced_wav = tmp.path().join("sliced_1s.wav");
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        ruvic::operations::extract_audio::execute_extract_audio(
            &backend,
            &cfg,
            ruvic::operations::extract_audio::ExtractAudioRequest {
                input: wav_in.clone(),
                output: sliced_mp3.clone(),
                to: Some("mp3".to_string()),
                copy: false,
                overwrite: true,
                audio_stream: None,
                bitrate: Some("320k".to_string()),
                start: Some("0.500".to_string()),
                duration: Some(1.0),
            },
            false,
        )
        .await
        .expect("sliced mp3");

        ruvic::operations::extract_audio::execute_extract_audio(
            &backend,
            &cfg,
            ruvic::operations::extract_audio::ExtractAudioRequest {
                input: wav_in.clone(),
                output: sliced_wav.clone(),
                to: Some("wav".to_string()),
                copy: false,
                overwrite: true,
                audio_stream: None,
                bitrate: None,
                start: Some("0.500".to_string()),
                duration: Some(1.0),
            },
            false,
        )
        .await
        .expect("sliced wav");
    });

    let (_, _, sliced_samples, sliced_rms) = decode_mp3_stats(&std::fs::read(&sliced_mp3).unwrap());
    // ~1s stereo 44.1kHz = ~88,200 samples (+ encoder padding frames)
    assert!(
        (70_000..=115_000).contains(&sliced_samples),
        "expected ~1s sliced MP3 samples (~88200), got {sliced_samples}"
    );
    assert!(sliced_rms > 0.05);

    let wav_info = probe_json(tmp.path(), &sliced_wav);
    assert_eq!(wav_info.container.format_name, "wav");
    let wav_dur = wav_info.container.duration_seconds.unwrap_or(0.0);
    assert!(
        (wav_dur - 1.0).abs() <= 0.05,
        "expected 1.0s sliced WAV, got {wav_dur}"
    );
    assert_no_temp_files(tmp.path());
}
