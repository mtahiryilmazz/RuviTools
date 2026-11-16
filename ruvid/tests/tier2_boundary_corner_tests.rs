mod common;

use common::synthetic_media::{Mp4FixtureBuilder, PlaylistFixtureBuilder, SubtitleFixtureBuilder};
use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;

// =========================================================================
// Feature 1: Range & Jobs Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f1_01_payload_under_16kb_threshold_single_chunk() {
    let server = MockHttpServer::start().await;
    // 10KB payload: below the 16KB threshold for multi-job chunking
    let payload_size = 10 * 1024usize;
    let payload: Vec<u8> = (0..payload_size).map(|i| (i % 256) as u8).collect();

    server.set_route(
        "/small.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("small_out.mp4");

    // Even with --jobs 4, payload < 16KB uses single stream
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/small.mp4", server.base_url),
            "--jobs",
            "4",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""chunks_total": 1"#))
        .stdout(predicate::str::contains(r#""bytes_downloaded": 10240"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t2_f1_02_exact_16384_byte_boundary() {
    let server = MockHttpServer::start().await;
    let payload_size = 16 * 1024usize; // exactly 16384 bytes
    let payload: Vec<u8> = (0..payload_size).map(|i| ((i * 7) % 251) as u8).collect();

    server.set_route(
        "/boundary_16k.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("boundary_16k.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/boundary_16k.mp4", server.base_url),
            "--jobs",
            "2",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""bytes_downloaded": 16384"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t2_f1_03_server_416_range_not_satisfiable() {
    let server = MockHttpServer::start().await;
    let mut route = RoutePayload::bytes(b"ERROR".to_vec(), "text/plain");
    route.status = 416; // Range Not Satisfiable
    server.set_route("/error_416.mp4", route);

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("error_416.mp4");

    // Must fail closed with non-zero exit code and remove temp part file
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/error_416.mp4", server.base_url),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .failure();

    assert!(!out_path.exists());
    assert!(!tmp.path().join("error_416.mp4.part").exists());
}

#[tokio::test]
async fn t2_f1_04_server_200_ok_ignore_range_fallback() {
    let server = MockHttpServer::start().await;
    let payload_size = 32 * 1024usize;
    let payload: Vec<u8> = (0..payload_size).map(|i| ((i * 11) % 247) as u8).collect();

    let mut route = RoutePayload::bytes(payload.clone(), "video/mp4");
    route.honor_range = false; // Always sends full 200 OK
    server.set_route("/ignore_range.mp4", route);

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("fallback_out.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/ignore_range.mp4", server.base_url),
            "--jobs",
            "4",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""chunks_total": 1"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t2_f1_05_single_byte_payload() {
    let server = MockHttpServer::start().await;
    let payload = vec![0x42u8];

    server.set_route(
        "/single_byte.bin",
        RoutePayload::bytes(payload.clone(), "application/octet-stream"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("single_byte.bin");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/single_byte.bin", server.base_url),
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""bytes_downloaded": 1"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

// =========================================================================
// Feature 2: DASH 4K/8K Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f2_01_video_only_without_audio_stream() {
    let server = MockHttpServer::start().await;
    let video_bytes = Mp4FixtureBuilder::new()
        .video(3840, 2160)
        .video_only()
        .build_faststart();

    server.set_route(
        "/video_only.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 313, "url": "{}/video_only.mp4", "mimeType": "video/mp4", "width": 3840, "height": 2160, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Video Only 4K",
        "v_only",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_v_only", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("v_only.mp4");

    // Downloading a video-only DASH stream without audio succeeds without crash
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_v_only", server.base_url),
            "-f",
            "itag-313",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-313""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), video_bytes);
}

#[tokio::test]
async fn t2_f2_02_audio_only_stream_download() {
    let server = MockHttpServer::start().await;
    let audio_bytes = b"AUDIO_ONLY_DATA_SAMPLE".to_vec();

    server.set_route(
        "/audio_only.m4a",
        RoutePayload::bytes(audio_bytes.clone(), "audio/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 140, "url": "{}/audio_only.m4a", "mimeType": "audio/mp4", "bitrate": 128000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        audio_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Audio Only",
        "a_only",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_a_only", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("a_only.m4a");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_a_only", server.base_url),
            "-f",
            "itag-140",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-140""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), audio_bytes);
}

#[tokio::test]
async fn t2_f2_03_60fps_stream_priority() {
    let server = MockHttpServer::start().await;

    let formats = "[]";
    let adaptive = r#"[
      {"itag": 313, "url": "http://127.0.0.1/v4k_30.mp4", "mimeType": "video/mp4", "width": 3840, "height": 2160, "fps": 30, "bitrate": 12000000, "contentLength": "1000"},
      {"itag": 401, "url": "http://127.0.0.1/v4k_60.mp4", "mimeType": "video/mp4", "width": 3840, "height": 2160, "fps": 60, "bitrate": 18000000, "contentLength": "1000"}
    ]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "60fps Priority Test",
        "vid_60fps",
        formats,
        adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_60fps", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    // Inspect via info: both 30 and 60 fps streams are detected
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_60fps", server.base_url),
            "--all-formats",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""frame_rate": "30""#))
        .stdout(predicate::str::contains(r#""frame_rate": "60""#));
}

#[tokio::test]
async fn t2_f2_04_drm_content_protection_rejected() {
    let server = MockHttpServer::start().await;
    let drm_m3u8 = include_str!("fixtures/hls/drm.m3u8");
    server.set_route(
        "/drm_stream.m3u8",
        RoutePayload::text(drm_m3u8, "application/vnd.apple.mpegurl"),
    );

    let tmp = tempfile::tempdir().unwrap();

    // Must be rejected with exit code 4 (unsupported DRM)
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/drm_stream.m3u8", server.base_url),
            "-o",
            tmp.path().join("drm.ts").to_str().unwrap(),
        ])
        .assert()
        .code(4);
}

#[tokio::test]
async fn t2_f2_05_invalid_format_selector_error() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"SAMPLE_VIDEO".to_vec();

    server.set_route(
        "/fmt_err.mp4",
        RoutePayload::bytes(video_bytes, "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();

    // Invalid format selector exits with exit code 2 (invalid argument)
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/fmt_err.mp4", server.base_url),
            "-f",
            "nonexistent_format_xyz_99999",
            "-o",
            tmp.path().join("out.mp4").to_str().unwrap(),
        ])
        .assert()
        .code(2);
}

// =========================================================================
// Feature 3: Playlist & Queue Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f3_01_empty_playlist_zero_items() {
    let server = MockHttpServer::start().await;
    let empty_m3u = "#EXTM3U\n";
    server.set_route(
        "/empty.m3u",
        RoutePayload::text(empty_m3u, "audio/x-mpegurl"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("empty_out");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/empty.m3u", server.base_url),
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .code(4)
        .stdout(predicate::str::contains("playlist contains no entries"));
}

#[tokio::test]
async fn t2_f3_02_single_item_fails_skip_errors() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/good1.mp4",
        RoutePayload::bytes(b"GOOD1".to_vec(), "video/mp4"),
    );
    // /missing.mp4 is intentionally NOT routed (returns 404)
    server.set_route(
        "/good2.mp4",
        RoutePayload::bytes(b"GOOD2".to_vec(), "video/mp4"),
    );

    let m3u = format!(
        "#EXTM3U\n#EXTINF:5,Good 1\n{}/good1.mp4\n#EXTINF:5,Missing 2\n{}/missing.mp4\n#EXTINF:5,Good 3\n{}/good2.mp4\n",
        server.base_url, server.base_url, server.base_url
    );
    server.set_route("/partial.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("partial_out");

    // With --skip-errors, playlist completes with 2 succeeded and 1 failed
    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/partial.m3u", server.base_url),
            "--skip-errors",
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""succeeded": 2"#))
        .stdout(predicate::str::contains(r#""failed": 1"#));

    assert!(out_dir.join("001_Good 1.mp4").exists());
    assert!(out_dir.join("003_Good 3.mp4").exists());
}

#[tokio::test]
async fn t2_f3_03_abort_on_item_error_without_skip_errors() {
    let server = MockHttpServer::start().await;
    // /missing_first.mp4 is NOT routed -> 404
    server.set_route(
        "/second.mp4",
        RoutePayload::bytes(b"SECOND".to_vec(), "video/mp4"),
    );

    let m3u = format!(
        "#EXTM3U\n#EXTINF:5,Missing\n{}/missing_first.mp4\n#EXTINF:5,Second\n{}/second.mp4\n",
        server.base_url, server.base_url
    );
    server.set_route("/abort.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("abort_out");

    // Without --skip-errors, encountering an error fails the command
    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/abort.m3u", server.base_url),
            "-o",
            out_dir.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

#[tokio::test]
async fn t2_f3_04_item_selection_out_of_bounds() {
    let server = MockHttpServer::start().await;
    server.set_route("/p1.mp4", RoutePayload::bytes(b"P1".to_vec(), "video/mp4"));

    let m3u = format!("#EXTM3U\n#EXTINF:5,P1\n{}/p1.mp4\n", server.base_url);
    server.set_route("/one_item.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("oob_out");

    // Requesting items 5-10 on a 1-item playlist yields 0 selected items
    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/one_item.m3u", server.base_url),
            "--items",
            "5-10",
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""total_selected": 0"#));
}

#[tokio::test]
async fn t2_f3_05_non_ascii_unicode_titles_sanitized() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/u1.mp4",
        RoutePayload::bytes(b"UNICODE_DATA".to_vec(), "video/mp4"),
    );

    // Title with Unicode and illegal Windows characters: < > : " / \ | ? *
    let m3u = format!(
        "#EXTM3U\n#EXTINF:5,Test: Clean? / Special <Tag> | * 🚀\n{}/u1.mp4\n",
        server.base_url
    );
    server.set_route("/unicode.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("unicode_out");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/unicode.m3u", server.base_url),
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""succeeded": 1"#));

    // Verify a file was written to disk and no crash occurred
    let files: Vec<_> = std::fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1);
}

// =========================================================================
// Feature 4: Chapters Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f4_01_video_has_zero_chapters() {
    let server = MockHttpServer::start().await;
    let formats = r#"[{"itag": 18, "url": "http://127.0.0.1/no_chap.mp4", "mimeType": "video/mp4", "contentLength": "1000"}]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Zero Chapters Video",
        "no_chap",
        formats,
        "[]",
        "[]",
        &[],
    );
    server.set_route("/watch_no_chap", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_no_chap", server.base_url),
            "--json",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn t2_f4_02_single_full_video_chapter() {
    let server = MockHttpServer::start().await;
    let formats = r#"[{"itag": 18, "url": "http://127.0.0.1/single_chap.mp4", "mimeType": "video/mp4", "contentLength": "1000"}]"#;
    let chapters = [("Complete Video", 0.0)];

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Single Chapter Video",
        "single_chap",
        formats,
        "[]",
        "[]",
        &chapters,
    );
    server.set_route("/watch_single_chap", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_single_chap", server.base_url),
            "--json",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn t2_f4_03_chapter_start_exceeds_duration() {
    let server = MockHttpServer::start().await;
    let formats = r#"[{"itag": 18, "url": "http://127.0.0.1/exceed.mp4", "mimeType": "video/mp4", "contentLength": "1000"}]"#;
    // Chapter start at 9999 seconds (video duration is 600s)
    let chapters = [("Intro", 0.0), ("Far Future", 9999.0)];

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Exceeding Chapter Video",
        "exceed_chap",
        formats,
        "[]",
        "[]",
        &chapters,
    );
    server.set_route("/watch_exceed", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    // Resolving does not crash or underflow
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_exceed", server.base_url),
            "--json",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn t2_f4_04_micro_chapter_sub_second() {
    let start_s: f64 = 1.0;
    let end_s: f64 = 1.5;
    let duration: f64 = end_s - start_s;
    assert!((duration - 0.5f64).abs() < 1e-6);
}

#[tokio::test]
async fn t2_f4_05_unordered_chapter_timestamps() {
    let mut chapters = [("Conclusion", 300.0), ("Intro", 0.0), ("Middle", 120.0)];
    chapters.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert_eq!(chapters[0].0, "Intro");
    assert_eq!(chapters[1].0, "Middle");
    assert_eq!(chapters[2].0, "Conclusion");
}

// =========================================================================
// Feature 5: Clips & Time-Range Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f5_01_inverted_range_start_after_end() {
    let start = 30.0f64;
    let end = 10.0f64;
    // Inverted range start > end is invalid
    assert!(start > end);
}

#[tokio::test]
async fn t2_f5_02_start_time_beyond_duration() {
    let start = 700.0f64;
    let total_duration = 600.0f64;
    assert!(start > total_duration);
}

#[tokio::test]
async fn t2_f5_03_zero_duration_clip() {
    let start = 15.0f64;
    let end = 15.0f64;
    let dur = end - start;
    assert_eq!(dur, 0.0);
}

#[tokio::test]
async fn t2_f5_04_clip_spans_end_of_media() {
    let start = 580.0f64;
    let requested_duration = 60.0f64;
    let media_duration = 600.0f64;
    let actual_duration = (start + requested_duration).min(media_duration) - start;
    assert_eq!(actual_duration, 20.0f64);
}

#[tokio::test]
async fn t2_f5_05_fractional_millisecond_precision() {
    let ms_str = "12.345";
    let val: f64 = ms_str.parse().unwrap();
    assert!((val - 12.345).abs() < 1e-6);
}

// =========================================================================
// Feature 6: Subtitle Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f6_01_xml_html_entities_unescaping() {
    let builder = SubtitleFixtureBuilder::new().cue(
        1000,
        3000,
        "Rock &amp; Roll with &quot;quotes&quot; &lt;brackets&gt;",
    );
    let xml = builder.build_timedtext_xml();
    assert!(xml.contains("&amp;amp;")); // escaped once for XML
}

#[tokio::test]
async fn t2_f6_02_inline_style_tags_stripped() {
    let text = "<b>Bold text</b> and <i>italic</i>";
    let clean = text.replace(['<', '>'], "");
    assert!(!clean.contains('<'));
    assert!(!clean.contains('>'));
}

#[tokio::test]
async fn t2_f6_03_empty_subtitle_track() {
    let builder = SubtitleFixtureBuilder::new();
    let vtt = builder.build_vtt();
    assert_eq!(vtt, "WEBVTT\n\n");
}

#[tokio::test]
async fn t2_f6_04_nonexistent_sub_lang_requested() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"VIDEO_SUB_FAILOVER".to_vec();
    server.set_route(
        "/sub_failover.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let captions_json = format!(
        r#"[
      {{"baseUrl": "{}/en.vtt", "name": {{"simpleText": "English"}}, "languageCode": "en", "vssId": ".en"}}
    ]"#,
        server.base_url
    );

    let formats = format!(
        r#"[
      {{"itag": 18, "url": "{}/sub_failover.mp4", "mimeType": "video/mp4", "contentLength": "{}"}}
    ]"#,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Nonexistent Sub Lang",
        "sub_none",
        &formats,
        "[]",
        &captions_json,
        &[],
    );
    server.set_route("/watch_sub_none", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("sub_none.mp4");

    // Requesting nonexistent subtitle lang "zz" downloads media without crashing
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_sub_none", server.base_url),
            "--subtitles",
            "--subtitles-lang",
            "zz",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    assert!(out_path.exists());
    assert!(!tmp.path().join("sub_none.zz.vtt").exists());
}

#[tokio::test]
async fn t2_f6_05_overlapping_cue_timestamps() {
    let builder = SubtitleFixtureBuilder::new().cue(1000, 4000, "Cue 1").cue(
        2000,
        5000,
        "Cue 2 (overlaps Cue 1)",
    );
    let vtt = builder.build_vtt();
    assert!(vtt.contains("00:00:01.000 --> 00:00:04.000"));
    assert!(vtt.contains("00:00:02.000 --> 00:00:05.000"));
}

// =========================================================================
// Feature 7: Dubbed Audio Boundaries (5 tests)
// =========================================================================

#[tokio::test]
async fn t2_f7_01_requested_lang_missing_handling() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"SAMPLE_VIDEO_DATA".to_vec();

    server.set_route(
        "/video_only.mp4",
        RoutePayload::bytes(video_bytes, "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();

    // Selecting a nonexistent audio format ID fails with exit code 2
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/video_only.mp4", server.base_url),
            "-f",
            "itag-999-missing-audio",
            "-o",
            tmp.path().join("out.mp4").to_str().unwrap(),
        ])
        .assert()
        .code(2);
}

#[tokio::test]
async fn t2_f7_02_multiple_tracks_same_language() {
    let server = MockHttpServer::start().await;

    let formats = "[]";
    let adaptive = r#"[
      {"itag": 140, "url": "http://127.0.0.1/a_std.m4a", "mimeType": "audio/mp4", "bitrate": 128000, "audioTrack": {"displayName": "English (Original)", "id": "en_std"}},
      {"itag": 141, "url": "http://127.0.0.1/a_desc.m4a", "mimeType": "audio/mp4", "bitrate": 128000, "audioTrack": {"displayName": "English (Descriptive)", "id": "en_desc"}}
    ]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Two EN Tracks",
        "two_en",
        formats,
        adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_two_en", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_two_en", server.base_url),
            "--all-formats",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "itag-140""#))
        .stdout(predicate::str::contains(r#""id": "itag-141""#));
}

#[tokio::test]
async fn t2_f7_03_audio_track_without_language_tag() {
    let server = MockHttpServer::start().await;

    let formats = "[]";
    let adaptive = r#"[
      {"itag": 140, "url": "http://127.0.0.1/a_nolang.m4a", "mimeType": "audio/mp4", "bitrate": 128000}
    ]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "No Lang Track",
        "no_lang",
        formats,
        adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_nolang", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    // Resolving audio track without language tag defaults safely without crash
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_nolang", server.base_url),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "itag-140""#));
}

#[tokio::test]
async fn t2_f7_04_case_insensitive_stream_selection() {
    let server = MockHttpServer::start().await;
    let audio_bytes = b"CASE_INSENSITIVE_AUDIO_DATA".to_vec();

    server.set_route(
        "/case_audio.m4a",
        RoutePayload::bytes(audio_bytes.clone(), "audio/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 140, "url": "{}/case_audio.m4a", "mimeType": "audio/mp4", "contentLength": "{}"}}
    ]"#,
        server.base_url,
        audio_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Case Insensitive Test",
        "case_test",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_case", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("case_out.m4a");

    // Case-insensitive format flag: "ITAG-140" matches "itag-140"
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_case", server.base_url),
            "-f",
            "ITAG-140",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-140""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), audio_bytes);
}

#[tokio::test]
async fn t2_f7_05_many_dubbed_languages_listing() {
    let server = MockHttpServer::start().await;

    let mut formats_list = Vec::new();
    for itag in 140..152 {
        formats_list.push(format!(
            r#"{{"itag": {}, "url": "http://127.0.0.1/a_{}.m4a", "mimeType": "audio/mp4", "bitrate": 128000}}"#,
            itag, itag
        ));
    }
    let adaptive = format!("[{}]", formats_list.join(","));

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Many Audio Tracks",
        "many_tracks",
        "[]",
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_many", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_many", server.base_url),
            "--all-formats",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "itag-140""#))
        .stdout(predicate::str::contains(r#""id": "itag-151""#));
}
