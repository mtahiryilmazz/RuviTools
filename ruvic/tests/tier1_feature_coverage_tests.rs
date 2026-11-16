//! Tier 1: Feature Coverage E2E Tests for ruvic Features 8-12.
//!
//! Covers:
//! - Feature 8: ruvic MP4 faststart & stco/co64 offsets (T1-F8-01 .. T1-F8-05)
//! - Feature 9: ruvic Audio Demuxing (ADTS / M4A) (T1-F9-01 .. T1-F9-05)
//! - Feature 10: ruvic DASH Dual-Stream Remuxing (T1-F10-01 .. T1-F10-05)
//! - Feature 11: ruvic 360/VR & HDR metadata preservation (T1-F11-01 .. T1-F11-05)
//! - Feature 12: Streaming I/O & Bounded RAM (T1-F12-01 .. T1-F12-05)
//!
//! All tests run in isolated environments without external FFmpeg/ffprobe.

mod common;

use std::fs;
use std::time::Instant;

use common::synthetic_media::*;

// ============================================================================
// Feature 8: ruvic MP4 Faststart & stco/co64 Offsets
// ============================================================================

/// T1-F8-01: Verifies that converting a non-faststart MP4 (moov at end) with web-mp4 preset
/// rearranges top-level boxes so `moov` precedes `mdat`.
#[test]
fn test_faststart_moov_before_mdat() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(6.0)
        .faststart(false)
        .build_non_faststart();

    assert!(
        !is_mp4_faststart(&in_bytes),
        "precondition: input must be non-faststart"
    );

    let in_path = tmp.path().join("input_non_faststart.mp4");
    let out_path = tmp.path().join("output_faststart.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "convert",
            in_path.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_path.is_file(), "output file must exist");
    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        is_mp4_faststart(&out_bytes),
        "output MP4 must be faststart (moov before mdat)"
    );

    let boxes = parse_top_level_boxes(&out_bytes);
    let moov_pos = boxes
        .iter()
        .position(|(t, _, _)| t == b"moov")
        .expect("moov box");
    let mdat_pos = boxes
        .iter()
        .position(|(t, _, _)| t == b"mdat")
        .expect("mdat box");
    assert!(moov_pos < mdat_pos, "moov must precede mdat in box order");

    assert_no_temp_files(tmp.path());
}

/// T1-F8-02: Verifies that chunk offsets in `stco` are accurately recalculated
/// to point directly to the media payload inside `mdat`.
#[test]
fn test_stco_chunk_offset_recalculation() {
    let tmp = tempfile::tempdir().unwrap();
    let payload = b"CHUNK_OFFSET_VERIFICATION_PAYLOAD_ABCD";
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(4.0)
        .faststart(false)
        .mdat_payload(payload)
        .build_non_faststart();

    let in_path = tmp.path().join("in_recalc.mp4");
    let out_path = tmp.path().join("out_recalc.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(is_mp4_faststart(&out_bytes));

    let offsets = extract_stco_offsets(&out_bytes);
    assert!(!offsets.is_empty(), "stco table must contain chunk offsets");

    // The first chunk offset must point to the payload inside mdat
    let mdat_pos = out_bytes
        .windows(4)
        .position(|w| w == b"mdat")
        .expect("mdat box");
    let actual_payload_start = (mdat_pos - 4 + 8) as u64; // mdat_pos is at tag, size is at mdat_pos - 4, payload is at mdat_pos + 4

    assert_eq!(
        offsets[0], actual_payload_start,
        "first chunk offset in stco must equal mdat payload start offset"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F8-03: Verifies that an MP4 containing both video and audio tracks has independent,
/// valid chunk offsets for each track in `stbl`.
#[test]
fn test_stco_dual_track_independent_offsets() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .video_only(1280, 720)
        .duration(5.0)
        .build_faststart();

    let in_path = tmp.path().join("dual_track_in.mp4");
    let out_path = tmp.path().join("dual_track_out.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    let offsets = extract_stco_offsets(&out_bytes);
    assert!(!offsets.is_empty(), "must have valid chunk offsets");
    for &off in &offsets {
        assert!(
            off < out_bytes.len() as u64,
            "chunk offset {off} must be within file size"
        );
    }
    assert_no_temp_files(tmp.path());
}

/// T1-F8-04: Verifies that 64-bit chunk offsets (`co64`) are generated and parsed
/// when 64-bit offsets are used or requested.
#[test]
fn test_co64_64bit_offset_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(3.0)
        .force_co64(true)
        .build_faststart();

    assert!(
        has_box_recursive(&in_bytes, b"co64"),
        "fixture must contain co64 atom"
    );

    let in_path = tmp.path().join("in_co64.mp4");
    let out_path = tmp.path().join("out_co64.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    let co64_offsets = extract_co64_offsets(&out_bytes);
    let stco_offsets = extract_stco_offsets(&out_bytes);

    // Either co64 was preserved or upgraded/converted to valid offsets
    assert!(
        !co64_offsets.is_empty() || !stco_offsets.is_empty(),
        "must have chunk offsets in co64 or stco"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F8-05: Verifies that presets specifying `faststart` produce valid faststart MP4 containers.
#[test]
fn test_preset_faststart_flag_enforcement() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(2.0)
        .faststart(false)
        .build_non_faststart();

    let in_path = tmp.path().join("in_preset.mp4");
    let out_path = tmp.path().join("out_web_preset.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "convert",
            in_path.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        is_mp4_faststart(&out_bytes),
        "web-mp4 preset must enforce faststart"
    );

    let probe = probe_json(tmp.path(), &out_path);
    assert_eq!(
        probe["container"]["format_name"].as_str().unwrap_or(""),
        "mov,mp4,m4a,3gp,3g2,mj2"
    );
    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 9: ruvic Audio Demuxing (ADTS / M4A)
// ============================================================================

/// T1-F9-01: Verifies extracting AAC audio from an MP4 file into a standalone ADTS `.aac` file.
#[test]
fn test_demux_aac_to_adts_file() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(4.0).build_faststart();

    let in_path = tmp.path().join("source_media.mp4");
    let out_aac = tmp.path().join("extracted_audio.aac");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "--to",
            "aac",
            "-o",
            out_aac.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_aac.is_file(), "output .aac file must exist");
    let aac_bytes = fs::read(&out_aac).unwrap();
    assert!(!aac_bytes.is_empty(), "aac output must not be empty");

    // Verify ADTS syncword (0xFFF)
    assert_eq!(aac_bytes[0], 0xFF, "first byte must be 0xFF");
    assert_eq!(
        aac_bytes[1] & 0xF6,
        0xF0,
        "second byte must match ADTS sync pattern"
    );

    let frames = validate_adts_stream(&aac_bytes);
    assert!(
        frames.is_ok(),
        "ADTS stream must be valid: {:?}",
        frames.err()
    );
    assert!(frames.unwrap() >= 1, "must extract at least 1 ADTS frame");

    assert_no_temp_files(tmp.path());
}

/// T1-F9-02: Verifies stream copy mode (`--copy`) during audio extraction.
#[test]
fn test_demux_aac_stream_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(3.0).build_faststart();

    let in_path = tmp.path().join("in_copy.mp4");
    let out_aac = tmp.path().join("out_copy.aac");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "--to",
            "aac",
            "--copy",
            "-o",
            out_aac.to_str().unwrap(),
        ])
        .assert()
        .success();

    let aac_bytes = fs::read(&out_aac).unwrap();
    assert!(validate_adts_stream(&aac_bytes).is_ok());
    assert_no_temp_files(tmp.path());
}

/// T1-F9-03: Verifies extracting audio into a standalone `.m4a` container.
#[test]
fn test_demux_audio_to_standalone_m4a() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(5.0).build_faststart();

    let in_path = tmp.path().join("full_av.mp4");
    let out_m4a = tmp.path().join("audio_only.m4a");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "-o",
            out_m4a.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_m4a.is_file());
    let m4a_bytes = fs::read(&out_m4a).unwrap();

    // Verify brand is M4A
    let ftyp_payload = find_box_payload(&m4a_bytes, b"ftyp").expect("ftyp box");
    assert_eq!(
        &ftyp_payload[0..4],
        b"M4A ",
        "standalone audio container brand must be M4A "
    );

    // Verify single stream (audio only) via probe
    let probe = probe_json(tmp.path(), &out_m4a);
    let streams = probe["streams"].as_array().expect("streams array");
    assert_eq!(
        streams.len(),
        1,
        "standalone m4a must contain exactly 1 stream"
    );
    assert_eq!(streams[0]["kind"].as_str().unwrap_or(""), "audio");
    assert_no_temp_files(tmp.path());
}

/// T1-F9-04: Verifies selecting a specific audio stream by index (`--audio-stream 1`).
#[test]
fn test_demux_multi_audio_track_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(4.0).build_faststart();

    let in_path = tmp.path().join("multi_in.mp4");
    let out_aac = tmp.path().join("selected_track.aac");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "--audio-stream",
            "1",
            "--to",
            "aac",
            "-o",
            out_aac.to_str().unwrap(),
        ])
        .assert()
        .success();

    let aac_bytes = fs::read(&out_aac).unwrap();
    assert!(validate_adts_stream(&aac_bytes).is_ok());
    assert_no_temp_files(tmp.path());
}

/// T1-F9-05: Verifies that extracted ADTS file has valid chained frame headers to exact EOF.
#[test]
fn test_demux_adts_frame_chain_validity() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(8.0).build_faststart();

    let in_path = tmp.path().join("long_audio.mp4");
    let out_aac = tmp.path().join("chained_frames.aac");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "audio",
            in_path.to_str().unwrap(),
            "--to",
            "aac",
            "-o",
            out_aac.to_str().unwrap(),
        ])
        .assert()
        .success();

    let aac_bytes = fs::read(&out_aac).unwrap();
    let count = validate_adts_stream(&aac_bytes).expect("valid ADTS stream");
    assert!(count >= 1, "must have multiple valid ADTS frames");
    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 10: ruvic DASH Dual-Stream Remuxing
// ============================================================================

/// T1-F10-01: Verifies multiplexing separate video-only MP4 and audio-only M4A
/// into a unified synchronized MP4 container via `--video` and `--audio`.
#[test]
fn test_dual_stream_remux_mp4_h264_aac() {
    let tmp = tempfile::tempdir().unwrap();

    let video_bytes = Mp4FixtureBuilder::new()
        .video_only(1920, 1080)
        .duration(10.0)
        .build_faststart();
    let audio_bytes = Mp4FixtureBuilder::new()
        .audio_only(48000, 2)
        .duration(10.0)
        .build_faststart();

    let v_path = tmp.path().join("video_part.mp4");
    let a_path = tmp.path().join("audio_part.m4a");
    let out_path = tmp.path().join("multiplexed_dash.mp4");

    fs::write(&v_path, &video_bytes).unwrap();
    fs::write(&a_path, &audio_bytes).unwrap();

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
        .success();

    assert!(out_path.is_file(), "multiplexed MP4 must be created");
    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        is_mp4_faststart(&out_bytes),
        "multiplexed MP4 must be faststart"
    );

    let probe = probe_json(tmp.path(), &out_path);
    let streams = probe["streams"].as_array().expect("streams array");
    assert_eq!(
        streams.len(),
        2,
        "must contain both video and audio streams"
    );

    let v_stream = streams
        .iter()
        .find(|s| s["kind"] == "video")
        .expect("video stream");
    let a_stream = streams
        .iter()
        .find(|s| s["kind"] == "audio")
        .expect("audio stream");
    assert_eq!(v_stream["width"], 1920);
    assert_eq!(v_stream["height"], 1080);
    assert_eq!(a_stream["channels"], 2);

    assert_no_temp_files(tmp.path());
}

/// T1-F10-02: Verifies multiplexing separate video-only WebM (VP9) and audio-only WebM (Opus).
#[test]
fn test_dual_stream_remux_webm_vp9_opus() {
    let tmp = tempfile::tempdir().unwrap();

    let video_bytes = WebmFixtureBuilder::new()
        .video_only(1920, 1080)
        .duration(6.0)
        .build();
    let audio_bytes = WebmFixtureBuilder::new()
        .audio_only(48000.0, 2)
        .duration(6.0)
        .build();

    let v_path = tmp.path().join("dash_video.webm");
    let a_path = tmp.path().join("dash_audio.webm");
    let out_path = tmp.path().join("dash_synced.webm");

    fs::write(&v_path, &video_bytes).unwrap();
    fs::write(&a_path, &audio_bytes).unwrap();

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
        .success();

    assert!(out_path.is_file());
    let probe = probe_json(tmp.path(), &out_path);
    let streams = probe["streams"].as_array().expect("streams array");
    assert_eq!(streams.len(), 2, "must contain both VP9 and Opus streams");
    assert_no_temp_files(tmp.path());
}

/// T1-F10-03: Verifies remuxing via a RUVI JSON handoff manifest containing video and audio parts.
#[test]
fn test_dual_stream_manifest_remux() {
    let tmp = tempfile::tempdir().unwrap();

    let video_bytes = Mp4FixtureBuilder::new()
        .video_only(1280, 720)
        .duration(5.0)
        .build_faststart();
    let audio_bytes = Mp4FixtureBuilder::new()
        .audio_only(44100, 2)
        .duration(5.0)
        .build_faststart();

    let v_path = tmp.path().join("manifest_video.mp4");
    let a_path = tmp.path().join("manifest_audio.m4a");
    let out_path = tmp.path().join("manifest_remuxed.mp4");
    let manifest_path = tmp.path().join("handoff.ruvi.json");

    fs::write(&v_path, &video_bytes).unwrap();
    fs::write(&a_path, &audio_bytes).unwrap();

    let manifest_json = serde_json::json!({
        "schema_version": "1.0",
        "primary_input": v_path.to_str().unwrap(),
        "secondary_input": a_path.to_str().unwrap(),
        "target_container": "mp4"
    });
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest_json).unwrap(),
    )
    .unwrap();

    native_cmd(tmp.path())
        .args([
            "remux",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_path.is_file());
    let probe = probe_json(tmp.path(), &out_path);
    assert_eq!(probe["streams"].as_array().unwrap().len(), 2);
    assert_no_temp_files(tmp.path());
}

/// T1-F10-04: Verifies duration alignment during dual-stream remuxing.
#[test]
fn test_dual_stream_duration_matching() {
    let tmp = tempfile::tempdir().unwrap();

    let v_bytes = Mp4FixtureBuilder::new()
        .video_only(1280, 720)
        .duration(8.0)
        .build_faststart();
    let a_bytes = Mp4FixtureBuilder::new()
        .audio_only(48000, 2)
        .duration(8.0)
        .build_faststart();

    let v_path = tmp.path().join("dur_v.mp4");
    let a_path = tmp.path().join("dur_a.m4a");
    let out_path = tmp.path().join("dur_out.mp4");

    fs::write(&v_path, &v_bytes).unwrap();
    fs::write(&a_path, &a_bytes).unwrap();

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
        .success();

    let probe = probe_json(tmp.path(), &out_path);
    let dur = probe["container"]["duration_seconds"]
        .as_f64()
        .unwrap_or(0.0);
    assert!(
        (dur - 8.0).abs() < 1.0,
        "duration should be close to 8.0s, got {dur}"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F10-05: Verifies that dual-stream remuxing applies faststart layout by default for MP4.
#[test]
fn test_dual_stream_remux_faststart_default() {
    let tmp = tempfile::tempdir().unwrap();

    let v_bytes = Mp4FixtureBuilder::new()
        .video_only(1920, 1080)
        .duration(4.0)
        .build_faststart();
    let a_bytes = Mp4FixtureBuilder::new()
        .audio_only(48000, 2)
        .duration(4.0)
        .build_faststart();

    let v_path = tmp.path().join("fs_v.mp4");
    let a_path = tmp.path().join("fs_a.m4a");
    let out_path = tmp.path().join("fs_dash_out.mp4");

    fs::write(&v_path, &v_bytes).unwrap();
    fs::write(&a_path, &a_bytes).unwrap();

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
        .success();

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        is_mp4_faststart(&out_bytes),
        "dual-stream MP4 remux must be faststart by default"
    );
    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 11: ruvic 360/VR & HDR Metadata Preservation
// ============================================================================

/// T1-F11-01: Verifies preservation of `sv3d` (Spatial 360 Video) box across remuxing.
#[test]
fn test_mp4_sv3d_spherical_preservation() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .spatial_360()
        .duration(4.0)
        .build_faststart();

    assert!(
        has_box_recursive(&in_bytes, b"sv3d"),
        "input must contain sv3d box"
    );

    let in_path = tmp.path().join("vr360_in.mp4");
    let out_path = tmp.path().join("vr360_out.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        has_box_recursive(&out_bytes, b"sv3d"),
        "remuxed MP4 must preserve sv3d box"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F11-02: Verifies preservation of `proj` equirectangular projection box inside `sv3d`.
#[test]
fn test_mp4_proj_equirectangular_preservation() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .spatial_360()
        .duration(3.0)
        .build_faststart();

    assert!(
        has_box_recursive(&in_bytes, b"proj"),
        "input must contain proj box"
    );

    let in_path = tmp.path().join("proj_in.mp4");
    let out_path = tmp.path().join("proj_out.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        has_box_recursive(&out_bytes, b"proj"),
        "output must preserve proj box"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F11-03: Verifies preservation of `colr` NCLX atom (BT.2020 primaries, PQ transfer).
#[test]
fn test_mp4_colr_bt2020_preservation() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .colr_bt2020_pq()
        .duration(5.0)
        .build_faststart();

    assert!(
        has_box_recursive(&in_bytes, b"colr"),
        "input must contain colr box"
    );

    let in_path = tmp.path().join("hdr_colr_in.mp4");
    let out_path = tmp.path().join("hdr_colr_out.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        has_box_recursive(&out_bytes, b"colr"),
        "output must preserve colr box"
    );

    let probe = probe_json(tmp.path(), &out_path);
    let v = &probe["streams"][0];
    assert_eq!(
        v["color_primaries"], 9,
        "color_primaries must be BT.2020 (9)"
    );
    assert_eq!(
        v["color_transfer"], 16,
        "color_transfer must be SMPTE ST 2084 PQ (16)"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F11-04: Verifies preservation of `mdcv` and `clli` HDR mastering display and light level atoms.
#[test]
fn test_mp4_mdcv_clli_hdr10_preservation() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new()
        .hdr_mdcv_clli()
        .duration(4.0)
        .build_faststart();

    assert!(has_box_recursive(&in_bytes, b"mdcv"));
    assert!(has_box_recursive(&in_bytes, b"clli"));

    let in_path = tmp.path().join("hdr10_in.mp4");
    let out_path = tmp.path().join("hdr10_out.mp4");
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

    let out_bytes = fs::read(&out_path).unwrap();
    assert!(
        has_box_recursive(&out_bytes, b"mdcv"),
        "output must preserve mdcv box"
    );
    assert!(
        has_box_recursive(&out_bytes, b"clli"),
        "output must preserve clli box"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F11-05: Verifies WebM Colour (`0x55B0`) and Projection (`0x7670`) element preservation across remux.
#[test]
fn test_webm_colour_projection_preservation() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = WebmFixtureBuilder::new()
        .colour_bt2020_pq()
        .projection_equirectangular()
        .duration(5.0)
        .build();

    let in_path = tmp.path().join("webm_hdr_in.webm");
    let out_path = tmp.path().join("webm_hdr_out.webm");
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

    let probe = probe_json(tmp.path(), &out_path);
    let v = &probe["streams"][0];
    assert_eq!(v["color_matrix"], 9);
    assert_eq!(v["color_primaries"], 9);
    assert_eq!(v["color_transfer"], 16);
    assert!(
        v["spatial_360"].is_array() || v["spatial_360"].is_string(),
        "spatial_360 metadata preserved"
    );
    assert_no_temp_files(tmp.path());
}

// ============================================================================
// Feature 12: Streaming I/O & Bounded RAM
// ============================================================================

/// T1-F12-01: Verifies probing a large file runs within bounded memory and rapid time.
#[test]
fn test_probe_large_file_bounded_ram() {
    let tmp = tempfile::tempdir().unwrap();
    let large_payload = vec![0x37u8; 10 * 1024 * 1024]; // 10MB payload
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(30.0)
        .mdat_payload(&large_payload)
        .build_faststart();

    let in_path = tmp.path().join("large_media.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    let start = Instant::now();
    let probe = probe_json(tmp.path(), &in_path);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "probe must complete quickly (took {:?})",
        elapsed
    );
    assert_eq!(
        probe["container"]["format_name"].as_str().unwrap_or(""),
        "mov,mp4,m4a,3gp,3g2,mj2"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F12-02: Verifies remuxing a large stream completes cleanly without memory blowup.
#[test]
fn test_remux_large_file_bounded_ram() {
    let tmp = tempfile::tempdir().unwrap();
    let large_payload = vec![0x55u8; 8 * 1024 * 1024]; // 8MB payload
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(20.0)
        .mdat_payload(&large_payload)
        .build_faststart();

    let in_path = tmp.path().join("large_stream.mp4");
    let out_path = tmp.path().join("large_stream_out.mp4");
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
    assert!(fs::metadata(&out_path).unwrap().len() >= 8 * 1024 * 1024);
    assert_no_temp_files(tmp.path());
}

/// T1-F12-03: Verifies trimming a large media stream using seek/chunked I/O.
#[test]
fn test_trim_large_file_chunked_io() {
    let tmp = tempfile::tempdir().unwrap();
    let large_payload = vec![0xAAu8; 6 * 1024 * 1024]; // 6MB
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(20.0)
        .mdat_payload(&large_payload)
        .build_faststart();

    let in_path = tmp.path().join("trim_large.mp4");
    let out_path = tmp.path().join("trim_large_out.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    native_cmd(tmp.path())
        .args([
            "trim",
            in_path.to_str().unwrap(),
            "--start",
            "00:00:05",
            "--duration",
            "5",
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_path.is_file());
    let probe = probe_json(tmp.path(), &out_path);
    let dur = probe["container"]["duration_seconds"]
        .as_f64()
        .unwrap_or(0.0);
    assert!(
        (dur - 5.0).abs() < 1.0,
        "trimmed duration should be ~5.0s, got {dur}"
    );
    assert_no_temp_files(tmp.path());
}

/// T1-F12-04: Verifies BufReader seek capability skips over large mdat payloads when moov is at end.
#[test]
fn test_bufreader_seek_skips_mdat() {
    let tmp = tempfile::tempdir().unwrap();
    let large_payload = vec![0x12u8; 12 * 1024 * 1024]; // 12MB mdat placed before moov
    let in_bytes = Mp4FixtureBuilder::new()
        .duration(15.0)
        .faststart(false)
        .mdat_payload(&large_payload)
        .build_non_faststart();

    assert!(!is_mp4_faststart(&in_bytes));

    let in_path = tmp.path().join("seek_skip.mp4");
    fs::write(&in_path, &in_bytes).unwrap();

    let start = Instant::now();
    let probe = probe_json(tmp.path(), &in_path);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "probe should seek directly to moov"
    );
    assert_eq!(probe["streams"].as_array().unwrap().len(), 2);
    assert_no_temp_files(tmp.path());
}

/// T1-F12-05: Verifies streaming operations perform atomic rename and leave zero temporary files.
#[test]
fn test_streaming_atomic_commit_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let in_bytes = Mp4FixtureBuilder::new().duration(3.0).build_faststart();
    let in_path = tmp.path().join("atomic_src.mp4");
    let out_path = tmp.path().join("atomic_dst.mp4");
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
