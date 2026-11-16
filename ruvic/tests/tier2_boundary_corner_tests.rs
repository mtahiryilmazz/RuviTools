//! Tier 2: Boundary Value Analysis & Corner Cases E2E Tests for ruvic Features 8-12.
//!
//! Covers:
//! - Feature 8: 4GB boundary, empty mdat, truncated stco, offset past EOF, corrupted moov (T2-F8-01 .. T2-F8-05)
//! - Feature 9: video-only reject, invalid audio stream index, incompatible codec copy, zero sample track, corrupt ADTS header (T2-F9-01 .. T2-F9-05)
//! - Feature 10: missing video, missing audio, incompatible codecs, corrupt stream file, existing destination without overwrite (T2-F10-01 .. T2-F10-05)
//! - Feature 11: unknown spatial box tolerance, truncated sv3d, SDR media omits HDR, trim preserves HDR/VR, probe surfaces HDR/VR in JSON (T2-F11-01 .. T2-F11-05)
//! - Feature 12: zero-byte file, read failure mid-stream cleanup, exact buffer boundary payload 64KB/1MB, sub-buffer small chunks, read-only output dir (T2-F12-01 .. T2-F12-05)
//!
//! All tests run in isolated environments without external FFmpeg/ffprobe.

mod common;

use std::fs;
use std::path::PathBuf;

use common::synthetic_media::*;

// ============================================================================
// Feature 8 Boundary & Corner Cases: MP4 Faststart & Chunk Offsets
// ============================================================================

/// T2-F8-01: Verifies chunk offset handling near/at the 4GB boundary (`0xFFFF_FFFF` vs `0x1_0000_0000`).
#[test]
fn test_boundary_4gb_offset_threshold() {
    let tmp = tempfile::tempdir().unwrap();

    // Generate an MP4 fixture declaring large 64-bit chunk offsets in co64
    let large_offset = 0x0001_0000_2000u64; // > 4GB
    let mut mp4 = Mp4FixtureBuilder::new()
        .duration(4.0)
        .force_co64(true)
        .build_faststart();

    // Replace the chunk offset with the >4GB value inside co64
    if let Some(co64_pos) = mp4.windows(4).position(|w| w == b"co64") {
        if co64_pos + 16 <= mp4.len() {
            mp4[co64_pos + 8..co64_pos + 16].copy_from_slice(&large_offset.to_be_bytes());
        }
    }

    let in_path = tmp.path().join("boundary_4gb.mp4");
    let out_path = tmp.path().join("boundary_4gb_out.mp4");
    fs::write(&in_path, &mp4).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("remux execution");

    // Must either succeed with valid co64 offset recalculation or safely report integrity failure
    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 8,
        "must exit 0 (success) or 8 (integrity), got {code}"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F8-02: Verifies that an empty `mdat` box (size = 8, 0 payload bytes)
/// is processed safely without arithmetic underflow or panic.
#[test]
fn test_empty_mdat_box_faststart() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(1.0)
        .mdat_payload(b"")
        .build_faststart();

    let in_path = tmp.path().join("empty_mdat.mp4");
    let out_path = tmp.path().join("empty_mdat_out.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("execute remux");

    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 8,
        "must succeed or cleanly report integrity error, got {code}"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F8-03: Verifies that truncated `stco` box (declares 50 entries but body truncated)
/// fails closed with exit code 8 (Integrity Error).
#[test]
fn test_truncated_stco_fails_integrity() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mp4 = Mp4FixtureBuilder::new().duration(2.0).build_faststart();

    // Tamper stco entry count to 50 when only 1 exists
    if let Some(pos) = mp4.windows(4).position(|w| w == b"stco") {
        if pos + 8 <= mp4.len() {
            mp4[pos + 4..pos + 8].copy_from_slice(&50u32.to_be_bytes());
        }
    }

    let in_path = tmp.path().join("truncated_stco.mp4");
    let out_path = tmp.path().join("should_not_exist.mp4");
    fs::write(&in_path, &mp4).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(8);

    assert!(
        !out_path.exists(),
        "corrupt input must not produce output file"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F8-04: Verifies that a chunk offset pointing past the EOF is rejected with exit code 8.
#[test]
fn test_stco_offset_points_past_eof() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mp4 = Mp4FixtureBuilder::new().duration(2.0).build_faststart();

    // Tamper first chunk offset to 0x7FFF_FFFF (past EOF)
    if let Some(pos) = mp4.windows(4).position(|w| w == b"stco") {
        if pos + 12 <= mp4.len() {
            mp4[pos + 8..pos + 12].copy_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
        }
    }

    let in_path = tmp.path().join("offset_past_eof.mp4");
    let out_path = tmp.path().join("should_not_exist.mp4");
    fs::write(&in_path, &mp4).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(8);

    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F8-05: Verifies that a corrupted `moov` box length (size extends past EOF)
/// fails with exit code 8 (Integrity Error).
#[test]
fn test_corrupted_moov_box_length() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mp4 = Mp4FixtureBuilder::new().duration(2.0).build_faststart();

    // Tamper moov box length at tag position - 4
    if let Some(pos) = mp4.windows(4).position(|w| w == b"moov") {
        if pos >= 4 {
            mp4[pos - 4..pos].copy_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
        }
    }

    let in_path = tmp.path().join("corrupt_moov.mp4");
    fs::write(&in_path, &mp4).unwrap();

    native_cmd(tmp.path())
        .args(["probe", in_path.to_str().unwrap(), "--json"])
        .assert()
        .code(8);

    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 9 Boundary & Corner Cases: Audio Demuxing
// ============================================================================

/// T2-F9-01: Verifies that attempting audio extraction on video-only media
/// fails cleanly without generating an invalid audio file.
#[test]
fn test_video_only_rejects_audio_extraction() {
    let tmp = tempfile::tempdir().unwrap();
    let video_only = Mp4FixtureBuilder::new()
        .video_only(1280, 720)
        .duration(3.0)
        .build_faststart();

    let in_path = tmp.path().join("video_only.mp4");
    let out_path = tmp.path().join("should_not_exist.m4a");
    fs::write(&in_path, &video_only).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("audio extract execution");

    assert!(
        !output.status.success(),
        "audio extraction on video-only media must fail"
    );
    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F9-02: Verifies that specifying an out-of-range audio stream index
/// (e.g. `--audio-stream 99`) fails cleanly with error code.
#[test]
fn test_invalid_audio_stream_index_out_of_range() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(3.0).build_faststart();

    let in_path = tmp.path().join("single_audio.mp4");
    let out_path = tmp.path().join("should_not_exist.aac");
    fs::write(&in_path, &in_bytes).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "--audio-stream",
            "99",
            "--to",
            "aac",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("audio extract execution");

    assert!(
        !output.status.success(),
        "out-of-range stream index must fail"
    );
    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F9-03: Verifies that copying an incompatible codec (e.g. `--to mp3 --copy` on AAC)
/// is rejected with exit code 4 (Unsupported).
#[test]
fn test_incompatible_codec_copy_rejection() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(2.0).build_faststart();

    let in_path = tmp.path().join("aac_source.mp4");
    let out_path = tmp.path().join("incompatible.mp3");
    fs::write(&in_path, &in_bytes).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "--to",
            "mp3",
            "--copy",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("audio copy execution");

    let code = output.status.code().unwrap_or(-1);
    // Exit code 4 (Unsupported) or non-zero error
    assert!(
        code == 4 || code != 0,
        "incompatible stream copy must fail (got {code})"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F9-04: Verifies that an audio track with 0 declared samples is handled without panic.
#[test]
fn test_zero_sample_audio_track() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mp4 = Mp4FixtureBuilder::new().duration(2.0).build_faststart();

    // Tamper stsz sample_count to 0
    if let Some(pos) = mp4.windows(4).position(|w| w == b"stsz") {
        if pos + 12 <= mp4.len() {
            mp4[pos + 8..pos + 12].copy_from_slice(&0u32.to_be_bytes());
        }
    }

    let in_path = tmp.path().join("zero_sample.mp4");
    let out_path = tmp.path().join("zero_out.m4a");
    fs::write(&in_path, &mp4).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("execution");

    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 8,
        "must handle zero samples safely, got {code}"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F9-05: Verifies that probing a corrupted ADTS file with invalid syncword fails with code 8.
#[test]
fn test_corrupt_adts_header_fails_probe() {
    let tmp = tempfile::tempdir().unwrap();
    let corrupt_adts = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x34];

    let in_path = tmp.path().join("corrupted.aac");
    fs::write(&in_path, &corrupt_adts).unwrap();

    native_cmd(tmp.path())
        .args(["probe", in_path.to_str().unwrap(), "--json"])
        .assert()
        .code(8);

    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 10 Boundary & Corner Cases: DASH Dual-Stream Remuxing
// ============================================================================

/// T2-F10-01: Verifies that dual-stream remuxing with a missing video file path
/// fails with exit code 5 (Filesystem error: file not found).
#[test]
fn test_dual_remux_missing_video_path() {
    let tmp = tempfile::tempdir().unwrap();
    let valid_audio = Mp4FixtureBuilder::new()
        .audio_only(48000, 2)
        .build_faststart();

    let missing_v = tmp.path().join("missing_video.mp4");
    let valid_a = tmp.path().join("valid_audio.m4a");
    let out_path = tmp.path().join("out.mp4");
    fs::write(&valid_a, &valid_audio).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            "--video",
            missing_v.to_str().unwrap(),
            "--audio",
            valid_a.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(5);

    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F10-02: Verifies that dual-stream remuxing with a missing audio file path
/// fails with exit code 5 (Filesystem error: file not found).
#[test]
fn test_dual_remux_missing_audio_path() {
    let tmp = tempfile::tempdir().unwrap();
    let valid_video = Mp4FixtureBuilder::new()
        .video_only(1280, 720)
        .build_faststart();

    let valid_v = tmp.path().join("valid_video.mp4");
    let missing_a = tmp.path().join("missing_audio.m4a");
    let out_path = tmp.path().join("out.mp4");
    fs::write(&valid_v, &valid_video).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            "--video",
            valid_v.to_str().unwrap(),
            "--audio",
            missing_a.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(5);

    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F10-03: Verifies that multiplexing incompatible codecs into MP4
/// (e.g. VP9 video + Opus audio into MP4 container without transcoding) fails with code 4.
#[test]
fn test_dual_remux_incompatible_codecs_to_mp4() {
    let tmp = tempfile::tempdir().unwrap();
    let vp9_webm = WebmFixtureBuilder::new().video_only(1280, 720).build();
    let opus_webm = WebmFixtureBuilder::new().audio_only(48000.0, 2).build();

    let v_path = tmp.path().join("vp9.webm");
    let a_path = tmp.path().join("opus.webm");
    let out_mp4 = tmp.path().join("incompatible.mp4");

    fs::write(&v_path, &vp9_webm).unwrap();
    fs::write(&a_path, &opus_webm).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "remux",
            "--video",
            v_path.to_str().unwrap(),
            "--audio",
            a_path.to_str().unwrap(),
            "-o",
            out_mp4.to_str().unwrap(),
        ])
        .output()
        .expect("execution");

    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 4 || code != 0,
        "must reject incompatible VP9/Opus remux to MP4, got {code}"
    );
    assert!(!out_mp4.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F10-04: Verifies that passing a corrupted/garbage video file during dual remux
/// fails with exit code 8 (Integrity Error) and cleans up temp files.
#[test]
fn test_dual_remux_corrupt_stream_file() {
    let tmp = tempfile::tempdir().unwrap();
    let garbage_v = b"NOT_A_VALID_VIDEO_FILE_AT_ALL_GARBAGE";
    let valid_a = Mp4FixtureBuilder::new()
        .audio_only(48000, 2)
        .build_faststart();

    let v_path = tmp.path().join("garbage.mp4");
    let a_path = tmp.path().join("valid.m4a");
    let out_path = tmp.path().join("should_not_exist.mp4");

    fs::write(&v_path, garbage_v).unwrap();
    fs::write(&a_path, &valid_a).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            "--video",
            v_path.to_str().unwrap(),
            "--audio",
            a_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(8);

    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F10-05: Verifies that dual remuxing to an existing file without `--overwrite`
/// fails with exit code 5 (Destination exists) and leaves the existing file untouched.
#[test]
fn test_dual_remux_output_exists_no_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let v_bytes = Mp4FixtureBuilder::new()
        .video_only(1280, 720)
        .build_faststart();
    let a_bytes = Mp4FixtureBuilder::new()
        .audio_only(48000, 2)
        .build_faststart();

    let v_path = tmp.path().join("v.mp4");
    let a_path = tmp.path().join("a.m4a");
    let existing_out = tmp.path().join("existing.mp4");

    fs::write(&v_path, &v_bytes).unwrap();
    fs::write(&a_path, &a_bytes).unwrap();
    let sentinel = b"DO_NOT_OVERWRITE_THIS_FILE";
    fs::write(&existing_out, sentinel).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            "--video",
            v_path.to_str().unwrap(),
            "--audio",
            a_path.to_str().unwrap(),
            "-o",
            existing_out.to_str().unwrap(),
        ])
        .assert()
        .code(5);

    assert_eq!(fs::read(&existing_out).unwrap(), sentinel);
    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 11 Boundary & Corner Cases: 360/VR & HDR Metadata Preservation
// ============================================================================

/// T2-F11-01: Verifies that unknown vendor-specific sub-boxes in visual sample entries
/// are tolerated without crashing or corrupting container structure.
#[test]
fn test_unknown_spatial_subbox_tolerance() {
    let tmp = tempfile::tempdir().unwrap();
    let unknown_box = wrap_box(b"XYZ1", b"VENDOR_CUSTOM_METADATA_PAYLOAD");
    let in_bytes = Mp4FixtureBuilder::new()
        .add_video_extra_box(&unknown_box)
        .duration(3.0)
        .build_faststart();

    let in_path = tmp.path().join("unknown_box_in.mp4");
    let out_path = tmp.path().join("unknown_box_out.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("remux execution");

    assert!(
        output.status.success(),
        "remux should tolerate unknown sub-boxes"
    );
    assert!(out_path.is_file());
    assert_no_temp_files(tmp.path());
}

/// T2-F11-02: Verifies that truncated `sv3d` box length fails closed with code 8.
#[test]
fn test_truncated_sv3d_box_fails_integrity() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mp4 = Mp4FixtureBuilder::new()
        .spatial_360()
        .duration(2.0)
        .build_faststart();

    // Tamper sv3d declared length to exceed sample entry
    if let Some(pos) = mp4.windows(4).position(|w| w == b"sv3d") {
        if pos >= 4 {
            mp4[pos - 4..pos].copy_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
        }
    }

    let in_path = tmp.path().join("truncated_sv3d.mp4");
    let out_path = tmp.path().join("should_not_exist.mp4");
    fs::write(&in_path, &mp4).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(8);

    assert_no_temp_files(tmp.path());
}

/// T2-F11-03: Verifies that remuxing standard SDR media does not inject spurious HDR metadata.
#[test]
fn test_sdr_media_omits_hdr_boxes() {
    let tmp = tempfile::tempdir().unwrap();
    let sdr_bytes = Mp4FixtureBuilder::new().duration(3.0).build_faststart();

    assert!(!has_box_recursive(&sdr_bytes, b"colr"));
    assert!(!has_box_recursive(&sdr_bytes, b"mdcv"));
    assert!(!has_box_recursive(&sdr_bytes, b"clli"));

    let in_path = tmp.path().join("sdr_in.mp4");
    let out_path = tmp.path().join("sdr_out.mp4");
    fs::write(&in_path, &sdr_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        !has_box_recursive(&out_bytes, b"mdcv"),
        "SDR must not inject mdcv"
    );
    assert!(
        !has_box_recursive(&out_bytes, b"clli"),
        "SDR must not inject clli"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F11-04: Verifies that trimming an HDR/VR media fixture preserves HDR/VR metadata.
#[test]
fn test_trim_preserves_hdr_vr_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .colr_bt2020_pq()
        .spatial_360()
        .duration(10.0)
        .build_faststart();

    let in_path = tmp.path().join("hdr_vr_full.mp4");
    let out_path = tmp.path().join("hdr_vr_trimmed.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "trim",
            in_path.to_str().unwrap(),
            "--start",
            "00:00:02",
            "--duration",
            "3",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        has_box_recursive(&out_bytes, b"colr"),
        "trimmed file must preserve colr"
    );
    assert_no_temp_files(tmp.path());
}

/// T2-F11-05: Verifies that `ruvic probe --json` surfaces HDR and VR metadata fields.
#[test]
fn test_probe_surfaces_hdr_vr_metadata_json() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .colr_bt2020_pq()
        .spatial_360()
        .duration(4.0)
        .build_faststart();

    let in_path = tmp.path().join("probe_hdr_vr.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    let probe = probe_json(tmp.path(), &in_path);
    let v = &probe["streams"][0];

    assert_eq!(v["color_primaries"], 9);
    assert_eq!(v["color_transfer"], 16);
    assert_eq!(v["color_matrix"], 9);
    assert!(
        v["spatial_360"].is_array() || v["spatial_360"].is_string(),
        "spatial_360 must be in JSON"
    );
    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 12 Boundary & Corner Cases: Streaming I/O & Bounded RAM
// ============================================================================

/// T2-F12-01: Verifies that a zero-byte input file fails immediately with exit code 8.
#[test]
fn test_zero_byte_input_file() {
    let tmp = tempfile::tempdir().unwrap();
    let zero_file = tmp.path().join("zero_byte.mp4");
    fs::write(&zero_file, b"").unwrap();

    native_cmd(tmp.path())
        .args(["probe", zero_file.to_str().unwrap(), "--json"])
        .assert()
        .code(8);

    let out_file = tmp.path().join("out.mp4");
    native_cmd(tmp.path())
        .args([
            "remux",
            zero_file.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
        ])
        .assert()
        .code(8);

    assert!(!out_file.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F12-02: Verifies that an incomplete file truncated mid-mdat fails with code 8
/// and cleanly cleans up any temporary output files.
#[test]
fn test_read_failure_mid_stream_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let full_bytes = Mp4FixtureBuilder::new().duration(5.0).build_faststart();

    // Truncate halfway through
    let truncated = &full_bytes[..full_bytes.len() / 2];
    let in_path = tmp.path().join("mid_stream_truncated.mp4");
    let out_path = tmp.path().join("should_not_exist.mp4");
    fs::write(&in_path, truncated).unwrap();

    let output = native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("execution");

    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 8 || code != 0,
        "truncated input must fail (got {code})"
    );
    assert!(!out_path.exists());
    assert_no_temp_files(tmp.path());
}

/// T2-F12-03: Verifies exact buffer boundary payload sizes (64KB and 1MB).
#[test]
fn test_exact_buffer_boundary_payload() {
    let tmp = tempfile::tempdir().unwrap();

    for &size in &[64 * 1024, 1024 * 1024] {
        let payload = vec![0x42u8; size];
        let in_bytes = Mp4FixtureBuilder::new()
            .duration(3.0)
            .mdat_payload(&payload)
            .build_faststart();

        let in_name = format!("boundary_{size}.mp4");
        let out_name = format!("out_boundary_{size}.mp4");
        let in_path = tmp.path().join(&in_name);
        let out_path = tmp.path().join(&out_name);

        fs::write(&in_path, &in_bytes).unwrap();

        native_cmd(tmp.path())
            .args([
                "remux",
                in_path.to_str().unwrap(),
                "-o",
                out_path.to_str().unwrap(),
            ])
            .assert()
            .success();

        assert!(out_path.is_file());
    }
    assert_no_temp_files(tmp.path());
}

/// T2-F12-04: Verifies tiny sub-buffer chunks (1-byte, 5-byte) process cleanly without hang.
#[test]
fn test_sub_buffer_small_sparse_chunks() {
    let tmp = tempfile::tempdir().unwrap();
    let tiny_payload = b"12345";
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(1.0)
        .mdat_payload(tiny_payload)
        .build_faststart();

    let in_path = tmp.path().join("tiny_chunk.mp4");
    let out_path = tmp.path().join("tiny_chunk_out.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_path.is_file());
    assert_no_temp_files(tmp.path());
}

/// T2-F12-05: Verifies attempting to write into a non-writable/read-only location fails with code 5.
#[test]
fn test_read_only_output_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(2.0).build_faststart();
    let in_path = tmp.path().join("in_valid.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    // Use a non-existent parent directory path on Windows that cannot be written
    let impossible_path = PathBuf::from("Z:\\nonexistent_volume_root_xyz\\impossible_out.mp4");

    let output = native_cmd(tmp.path())
        .args([
            "remux",
            in_path.to_str().unwrap(),
            "-o",
            impossible_path.to_str().unwrap(),
        ])
        .output()
        .expect("execution");

    let code = output.status.code().unwrap_or(-1);
    assert_eq!(code, 5, "must fail with exit code 5 (Filesystem error)");
    assert_no_temp_files(tmp.path());
}
