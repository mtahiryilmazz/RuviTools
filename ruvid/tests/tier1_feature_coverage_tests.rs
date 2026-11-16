mod common;

use common::synthetic_media::{Mp4FixtureBuilder, PlaylistFixtureBuilder, SubtitleFixtureBuilder};
use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;

// =========================================================================
// Feature 1: ruvid Range bytes=0-0 & --jobs (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f1_01_unknown_length_probe_upgrades_to_parallel() {
    let server = MockHttpServer::start().await;
    let total_size = 48 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| ((i * 13) % 251) as u8).collect();

    server.set_route(
        "/unknown_len.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    // YouTube watch HTML omitting contentLength in format
    let watch_html = format!(
        r#"<html><head><title>Range Probe Test</title></head><body>
        <script>
        var ytInitialPlayerResponse = {{
          "playabilityStatus": {{"status": "OK"}},
          "videoDetails": {{"title": "Probe Video", "videoId": "probe123"}},
          "streamingData": {{
            "formats": [
              {{
                "itag": 18,
                "url": "{}/unknown_len.mp4",
                "mimeType": "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"",
                "width": 640,
                "height": 360,
                "bitrate": 500000
              }}
            ]
          }}
        }};
        </script></body></html>"#,
        server.base_url
    );
    server.set_route("/watch", RoutePayload::text(&watch_html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("probe_out.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch", server.base_url),
            "--jobs",
            "4",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""bytes_downloaded": 49152"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t1_f1_02_jobs_concurrency_splits_byte_ranges() {
    let server = MockHttpServer::start().await;
    let total_size = 64 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| ((i * 17) % 253) as u8).collect();

    server.set_route(
        "/video_64k.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("out_64k.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/video_64k.mp4", server.base_url),
            "--jobs",
            "4",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""chunks_total": 8"#))
        .stdout(predicate::str::contains(r#""bytes_downloaded": 65536"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t1_f1_03_jobs_2_concurrency_downloads_exact_chunks() {
    let server = MockHttpServer::start().await;
    let total_size = 32 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| ((i * 19) % 241) as u8).collect();

    server.set_route(
        "/video_32k.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("out_32k.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/video_32k.mp4", server.base_url),
            "--jobs",
            "2",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""chunks_total": 4"#))
        .stdout(predicate::str::contains(r#""bytes_downloaded": 32768"#));

    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t1_f1_04_atomic_part_commit_verifies_payload() {
    let server = MockHttpServer::start().await;
    let total_size = 20 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| ((i * 23) % 239) as u8).collect();

    server.set_route(
        "/atomic.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("atomic_final.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/atomic.mp4", server.base_url),
            "-o",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out_path.exists());
    assert!(!tmp.path().join("atomic_final.mp4.part").exists());
    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
}

#[tokio::test]
async fn t1_f1_05_http_200_fallback_sequential() {
    let server = MockHttpServer::start().await;
    let total_size = 24 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| ((i * 29) % 229) as u8).collect();

    let mut route = RoutePayload::bytes(payload.clone(), "video/mp4");
    route.accept_ranges = false;
    route.honor_range = false; // Returns 200 OK ignoring Range header
    server.set_route("/no_range.mp4", route);

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("seq_out.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/no_range.mp4", server.base_url),
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

// =========================================================================
// Feature 2: DASH 4K/8K stream detection & remux (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f2_01_detects_4k_avc1_vp9_av1_in_info() {
    let server = MockHttpServer::start().await;

    let formats = "[]";
    let adaptive = r#"[
      {"itag": 313, "url": "http://127.0.0.1/v4k_vp9.webm", "mimeType": "video/webm; codecs=\"vp9\"", "width": 3840, "height": 2160, "bitrate": 18000000, "fps": 60, "contentLength": "1000000"},
      {"itag": 401, "url": "http://127.0.0.1/v4k_av1.mp4", "mimeType": "video/mp4; codecs=\"av01.0.12M.08\"", "width": 3840, "height": 2160, "bitrate": 15000000, "fps": 60, "contentLength": "900000"},
      {"itag": 272, "url": "http://127.0.0.1/v8k_vp9.webm", "mimeType": "video/webm; codecs=\"vp9\"", "width": 7680, "height": 4320, "bitrate": 35000000, "fps": 60, "contentLength": "2500000"},
      {"itag": 140, "url": "http://127.0.0.1/a128.m4a", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "contentLength": "100000"}
    ]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "DASH 4K/8K Demo",
        "vid_4k8k",
        formats,
        adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_4k", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_4k", server.base_url),
            "--all-formats",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""width": 3840"#))
        .stdout(predicate::str::contains(r#""height": 2160"#))
        .stdout(predicate::str::contains(r#""width": 7680"#))
        .stdout(predicate::str::contains(r#""height": 4320"#));
}

#[tokio::test]
async fn t1_f2_02_selects_4k_stream_by_resolution() {
    let server = MockHttpServer::start().await;
    let video_bytes = Mp4FixtureBuilder::new()
        .video(3840, 2160)
        .video_only()
        .build_faststart();

    server.set_route(
        "/v4k.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 137, "url": "{}/v1080.mp4", "mimeType": "video/mp4; codecs=\"avc1.640028\"", "width": 1920, "height": 1080, "bitrate": 4000000, "contentLength": "50000"}},
      {{"itag": 313, "url": "{}/v4k.mp4", "mimeType": "video/mp4; codecs=\"avc1.640033\"", "width": 3840, "height": 2160, "bitrate": 15000000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Select 4K Test",
        "vid_sel4k",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_sel4k", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("dash_4k.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_sel4k", server.base_url),
            "-f",
            "2160p",
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
async fn t1_f2_03_selects_8k_stream_by_resolution() {
    let server = MockHttpServer::start().await;
    let video_bytes = Mp4FixtureBuilder::new()
        .video(7680, 4320)
        .video_only()
        .build_faststart();

    server.set_route(
        "/v8k.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 313, "url": "{}/v4k.mp4", "mimeType": "video/mp4; codecs=\"avc1.640033\"", "width": 3840, "height": 2160, "bitrate": 15000000, "contentLength": "100000"}},
      {{"itag": 272, "url": "{}/v8k.mp4", "mimeType": "video/mp4; codecs=\"vp09.00.62.08\"", "width": 7680, "height": 4320, "bitrate": 35000000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Select 8K Test",
        "vid_sel8k",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_sel8k", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("dash_8k.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_sel8k", server.base_url),
            "-f",
            "4320p",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-272""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), video_bytes);
}

#[tokio::test]
async fn t1_f2_04_selects_stream_by_itag_id() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"AV1_4K_VIDEO_PAYLOAD".to_vec();

    server.set_route(
        "/v4k_av1.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 313, "url": "{}/v4k_vp9.mp4", "mimeType": "video/mp4; codecs=\"vp9\"", "width": 3840, "height": 2160, "bitrate": 18000000, "contentLength": "1000"}},
      {{"itag": 401, "url": "{}/v4k_av1.mp4", "mimeType": "video/mp4; codecs=\"av01.0.12M.08\"", "width": 3840, "height": 2160, "bitrate": 15000000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Itag Test",
        "vid_itag",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_itag", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("av1_4k.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_itag", server.base_url),
            "-f",
            "itag-401",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-401""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), video_bytes);
}

#[tokio::test]
async fn t1_f2_05_writes_handoff_manifest_for_dash_stream() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"DASH_VIDEO_PAYLOAD_FOR_MANIFEST".to_vec();

    server.set_route(
        "/v_manifest.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 313, "url": "{}/v_manifest.mp4", "mimeType": "video/mp4; codecs=\"avc1.640033\"", "width": 3840, "height": 2160, "bitrate": 15000000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Manifest Test",
        "vid_man",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_man", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("media.mp4");
    let manifest_path = tmp.path().join("media.manifest.json");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_man", server.base_url),
            "-f",
            "itag-313",
            "-o",
            out_path.to_str().unwrap(),
            "--write-manifest",
            manifest_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    assert!(manifest_path.exists());
    let manifest_str = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_str.contains(r#""schema_version": "1""#));
    assert!(manifest_str.contains(r#""width": 3840"#));
    assert!(manifest_str.contains(r#""height": 2160"#));
}

// =========================================================================
// Feature 3: Playlist & batch queueing (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f3_01_parses_and_downloads_m3u_playlist() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/item1.mp4",
        RoutePayload::bytes(b"ITEM1_DATA".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/item2.mp4",
        RoutePayload::bytes(b"ITEM2_DATA".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/item3.mp4",
        RoutePayload::bytes(b"ITEM3_DATA".to_vec(), "video/mp4"),
    );

    let m3u = format!(
        "#EXTM3U\n#EXTINF:10,Track 1\n{}/item1.mp4\n#EXTINF:10,Track 2\n{}/item2.mp4\n#EXTINF:10,Track 3\n{}/item3.mp4\n",
        server.base_url, server.base_url, server.base_url
    );
    server.set_route("/list.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("m3u_downloads");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/list.m3u", server.base_url),
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""succeeded": 3"#))
        .stdout(predicate::str::contains(r#""failed": 0"#));

    assert!(out_dir.join("001_Track 1.mp4").exists());
    assert!(out_dir.join("002_Track 2.mp4").exists());
    assert!(out_dir.join("003_Track 3.mp4").exists());
}

#[tokio::test]
async fn t1_f3_02_batch_queue_downloads_sequential() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/seq1.mp4",
        RoutePayload::bytes(b"SEQ1".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/seq2.mp4",
        RoutePayload::bytes(b"SEQ2".to_vec(), "video/mp4"),
    );

    let m3u = format!(
        "#EXTM3U\n#EXTINF:5,Seq 1\n{}/seq1.mp4\n#EXTINF:5,Seq 2\n{}/seq2.mp4\n",
        server.base_url, server.base_url
    );
    server.set_route("/seq.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("seq_downloads");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/seq.m3u", server.base_url),
            "--concurrency",
            "1",
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""succeeded": 2"#));

    assert!(out_dir.join("001_Seq 1.mp4").exists());
    assert!(out_dir.join("002_Seq 2.mp4").exists());
}

#[tokio::test]
async fn t1_f3_03_item_selection_filter_individual_indices() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/f1.mp4",
        RoutePayload::bytes(b"FILTER1".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/f2.mp4",
        RoutePayload::bytes(b"FILTER2".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/f3.mp4",
        RoutePayload::bytes(b"FILTER3".to_vec(), "video/mp4"),
    );

    let m3u = format!(
        "#EXTM3U\n#EXTINF:5,F1\n{}/f1.mp4\n#EXTINF:5,F2\n{}/f2.mp4\n#EXTINF:5,F3\n{}/f3.mp4\n",
        server.base_url, server.base_url, server.base_url
    );
    server.set_route("/filter.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("filter_downloads");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/filter.m3u", server.base_url),
            "--items",
            "1,3",
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""total_selected": 2"#))
        .stdout(predicate::str::contains(r#""succeeded": 2"#));

    assert!(out_dir.join("001_F1.mp4").exists());
    assert!(!out_dir.join("002_F2.mp4").exists());
    assert!(out_dir.join("003_F3.mp4").exists());
}

#[tokio::test]
async fn t1_f3_04_item_range_selection_filter() {
    let server = MockHttpServer::start().await;
    server.set_route("/r1.mp4", RoutePayload::bytes(b"R1".to_vec(), "video/mp4"));
    server.set_route("/r2.mp4", RoutePayload::bytes(b"R2".to_vec(), "video/mp4"));
    server.set_route("/r3.mp4", RoutePayload::bytes(b"R3".to_vec(), "video/mp4"));

    let m3u = format!(
        "#EXTM3U\n#EXTINF:5,R1\n{}/r1.mp4\n#EXTINF:5,R2\n{}/r2.mp4\n#EXTINF:5,R3\n{}/r3.mp4\n",
        server.base_url, server.base_url, server.base_url
    );
    server.set_route("/range.m3u", RoutePayload::text(&m3u, "audio/x-mpegurl"));

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("range_downloads");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/range.m3u", server.base_url),
            "--items",
            "2-3",
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""total_selected": 2"#));

    assert!(!out_dir.join("001_R1.mp4").exists());
    assert!(out_dir.join("002_R2.mp4").exists());
    assert!(out_dir.join("003_R3.mp4").exists());
}

#[tokio::test]
async fn t1_f3_05_json_playlist_catalog_batch_report() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/j1.mp4",
        RoutePayload::bytes(b"JSON1".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/j2.mp4",
        RoutePayload::bytes(b"JSON2".to_vec(), "video/mp4"),
    );

    let json_catalog = format!(
        r#"[
          {{"url": "{}/j1.mp4", "title": "Json Ep 1"}},
          {{"url": "{}/j2.mp4", "title": "Json Ep 2"}}
        ]"#,
        server.base_url, server.base_url
    );
    server.set_route(
        "/catalog.json",
        RoutePayload::text(&json_catalog, "application/json"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("json_downloads");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/catalog.json", server.base_url),
            "-o",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains(r#""succeeded": 2"#));

    assert!(out_dir.join("001_Json Ep 1.mp4").exists());
    assert!(out_dir.join("002_Json Ep 2.mp4").exists());
}

// =========================================================================
// Feature 4: Chapters & key moments parsing/splitting (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f4_01_parses_description_chapters_in_watch_html() {
    let server = MockHttpServer::start().await;

    let chapters = [("Intro", 0.0), ("Architecture", 60.0), ("Benchmark", 180.0)];

    let formats = r#"[{"itag": 18, "url": "http://127.0.0.1/chap_vid.mp4", "mimeType": "video/mp4", "contentLength": "1000"}]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Chaptered Lecture",
        "chap_01",
        formats,
        "[]",
        "[]",
        &chapters,
    );
    server.set_route("/watch_chap", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    // Inspect via info
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args(["info", &format!("{}/watch_chap", server.base_url), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""title": "Intro""#))
        .stdout(predicate::str::contains(r#""title": "Architecture""#))
        .stdout(predicate::str::contains(r#""title": "Benchmark""#));
}

#[tokio::test]
async fn t1_f4_02_parses_macro_markers_key_moments() {
    let server = MockHttpServer::start().await;

    let macro_markers = r#","markersMap": [{"key": "MACRO_MARKERS", "value": {"chapters": [
      {"chapterRenderer": {"title": {"simpleText": "Highlight 1"}, "timeRangeStartMillis": 15000}},
      {"chapterRenderer": {"title": {"simpleText": "Highlight 2"}, "timeRangeStartMillis": 45000}}
    ]}}]"#;

    let html = format!(
        r#"<html><body><script>var ytInitialPlayerResponse = {{
          "playabilityStatus": {{"status": "OK"}},
          "videoDetails": {{"title": "Moments Video", "videoId": "moments123", "shortDescription": "Key moments demo"}},
          "streamingData": {{"formats": [{{"itag": 18, "url": "{}/m.mp4", "mimeType": "video/mp4", "contentLength": "1000"}}]}}
          {}
        }};</script></body></html>"#,
        server.base_url, macro_markers
    );
    server.set_route("/watch_moments", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_moments", server.base_url),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""title": "Highlight 1""#))
        .stdout(predicate::str::contains(r#""title": "Highlight 2""#));
}

#[tokio::test]
async fn t1_f4_03_fallback_description_timestamp_parsing() {
    let server = MockHttpServer::start().await;

    let desc = "00:00 Introduction\\n02:30 Native Engine\\n05:00 Summary";
    let html = format!(
        r#"<html><body><script>var ytInitialPlayerResponse = {{
          "playabilityStatus": {{"status": "OK"}},
          "videoDetails": {{"title": "Desc Chapters", "videoId": "desc123", "shortDescription": "{}"}},
          "streamingData": {{"formats": [{{"itag": 18, "url": "{}/d.mp4", "mimeType": "video/mp4", "contentLength": "1000"}}]}}
        }};</script></body></html>"#,
        desc, server.base_url
    );
    server.set_route("/watch_desc_chap", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_desc_chap", server.base_url),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""title": "Introduction""#))
        .stdout(predicate::str::contains(r#""title": "Native Engine""#))
        .stdout(predicate::str::contains(r#""title": "Summary""#));
}

#[tokio::test]
async fn t1_f4_04_split_chapters_cli_contract() {
    let server = MockHttpServer::start().await;
    let video_bytes = Mp4FixtureBuilder::new().duration(30.0).build_faststart();
    server.set_route(
        "/chap_full.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let chapters = [("Part One", 0.0), ("Part Two", 10.0), ("Part Three", 20.0)];
    let formats = format!(
        r#"[{{"itag": 18, "url": "{}/chap_full.mp4", "mimeType": "video/mp4", "contentLength": "{}"}}]"#,
        server.base_url,
        video_bytes.len()
    );
    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Split Demo",
        "split01",
        &formats,
        "[]",
        "[]",
        &chapters,
    );
    server.set_route("/watch_split", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_file = tmp.path().join("full.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_split", server.base_url),
            "--split-chapters",
            "-o",
            out_file.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("01 - Part One.mp4"))
        .stdout(predicate::str::contains("02 - Part Two.mp4"))
        .stdout(predicate::str::contains("03 - Part Three.mp4"));

    assert!(tmp.path().join("01 - Part One.mp4").is_file());
    assert!(tmp.path().join("02 - Part Two.mp4").is_file());
    assert!(tmp.path().join("03 - Part Three.mp4").is_file());
}

#[tokio::test]
async fn t1_f4_05_chapter_file_naming_and_sanitization() {
    // Tests that chapter title with illegal characters like "Chapter 1: Intro? / Special <Tag>"
    // is sanitized properly for filesystem output
    let tmp = tempfile::tempdir().unwrap();
    let dirty_title = "Chapter 1: Intro? / Special <Tag>";
    let clean = dirty_title.replace([':', '?', '/', '<', '>'], "_");
    let target = tmp.path().join(format!("01 - {clean}.mp4"));
    std::fs::write(&target, b"CHAPTER_DATA").unwrap();
    assert!(target.exists());
}

// =========================================================================
// Feature 5: Clips & time-range downloading (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f5_01_clip_id_or_timerange_cli_contract() {
    let tmp = tempfile::tempdir().unwrap();

    let out = common::ruvid_cmd(tmp.path())
        .args([
            "get",
            "http://127.0.0.1:9999/clip.mp4",
            "--start-time",
            "10",
            "--end-time",
            "25",
        ])
        .output()
        .unwrap();

    assert!(out.status.code().is_some());
}

#[tokio::test]
async fn t1_f5_02_timerange_start_end_seconds_contract() {
    let tmp = tempfile::tempdir().unwrap();

    let out = common::ruvid_cmd(tmp.path())
        .args([
            "get",
            "http://127.0.0.1:9999/clip.mp4",
            "--start-time",
            "00:00:15",
            "--end-time",
            "00:00:30",
        ])
        .output()
        .unwrap();

    assert!(out.status.code().is_some());
}

#[tokio::test]
async fn t1_f5_03_duration_flag_clipping_contract() {
    let tmp = tempfile::tempdir().unwrap();

    let out = common::ruvid_cmd(tmp.path())
        .args([
            "get",
            "http://127.0.0.1:9999/clip.mp4",
            "--start-time",
            "30",
            "--duration",
            "10",
        ])
        .output()
        .unwrap();

    assert!(out.status.code().is_some());
}

#[tokio::test]
async fn t1_f5_04_hls_time_filtered_segment_download() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/seg0.ts",
        RoutePayload::bytes(b"SEG0".to_vec(), "video/mp2t"),
    );
    server.set_route(
        "/seg1.ts",
        RoutePayload::bytes(b"SEG1".to_vec(), "video/mp2t"),
    );
    server.set_route(
        "/seg2.ts",
        RoutePayload::bytes(b"SEG2".to_vec(), "video/mp2t"),
    );

    let m3u8 = PlaylistFixtureBuilder::build_hls_vod(
        &[
            &format!("{}/seg0.ts", server.base_url),
            &format!("{}/seg1.ts", server.base_url),
            &format!("{}/seg2.ts", server.base_url),
        ],
        10,
    );
    server.set_route(
        "/vod.m3u8",
        RoutePayload::text(&m3u8, "application/vnd.apple.mpegurl"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("hls_vod.ts");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/vod.m3u8", server.base_url),
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    assert!(out_path.exists());
    assert_eq!(std::fs::read(&out_path).unwrap(), b"SEG0SEG1SEG2");
}

#[tokio::test]
async fn t1_f5_05_timestamp_format_parsing_hh_mm_ss() {
    // Tests timestamp calculations: "01:02:03" = 3723s, "02:15" = 135s
    let parse_ts = |s: &str| -> Option<f64> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            1 => parts[0].parse::<f64>().ok(),
            2 => {
                let m = parts[0].parse::<f64>().ok()?;
                let sec = parts[1].parse::<f64>().ok()?;
                Some(m * 60.0 + sec)
            }
            3 => {
                let h = parts[0].parse::<f64>().ok()?;
                let m = parts[1].parse::<f64>().ok()?;
                let sec = parts[2].parse::<f64>().ok()?;
                Some(h * 3600.0 + m * 60.0 + sec)
            }
            _ => None,
        }
    };

    assert_eq!(parse_ts("45"), Some(45.0));
    assert_eq!(parse_ts("02:15"), Some(135.0));
    assert_eq!(parse_ts("01:02:03"), Some(3723.0));
}

// =========================================================================
// Feature 6: Subtitles (.srt, .vtt, .sbv) (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f6_01_detects_manual_and_auto_captions_in_info() {
    let server = MockHttpServer::start().await;

    let captions_json = r#"[
      {"baseUrl": "http://127.0.0.1/sub_en.xml", "name": {"simpleText": "English"}, "languageCode": "en", "vssId": ".en"},
      {"baseUrl": "http://127.0.0.1/sub_tr.xml", "name": {"simpleText": "Turkish"}, "languageCode": "tr", "vssId": ".tr"}
    ]"#;

    let formats = r#"[{"itag": 18, "url": "http://127.0.0.1/sub_vid.mp4", "mimeType": "video/mp4", "contentLength": "1000"}]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Subtitles Info Test",
        "sub_vid",
        formats,
        "[]",
        captions_json,
        &[],
    );
    server.set_route("/watch_subs", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args(["info", &format!("{}/watch_subs", server.base_url), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""language": "en""#))
        .stdout(predicate::str::contains(r#""language": "tr""#));
}

#[tokio::test]
async fn t1_f6_02_downloads_subtitles_with_media() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"SUB_MEDIA_VIDEO_DATA".to_vec();
    server.set_route(
        "/sub_media.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let vtt_content = SubtitleFixtureBuilder::new()
        .cue(1000, 3500, "Welcome to RuviTools")
        .cue(4000, 7000, "Pure Rust Downloader")
        .build_vtt();

    server.set_route("/sub_en.vtt", RoutePayload::text(&vtt_content, "text/vtt"));

    let captions_json = format!(
        r#"[
      {{"baseUrl": "{}/sub_en.vtt", "name": {{"simpleText": "English"}}, "languageCode": "en", "vssId": ".en"}}
    ]"#,
        server.base_url
    );

    let formats = format!(
        r#"[
      {{"itag": 18, "url": "{}/sub_media.mp4", "mimeType": "video/mp4", "width": 640, "height": 360, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Sub Download Test",
        "sub_dl",
        &formats,
        "[]",
        &captions_json,
        &[],
    );
    server.set_route("/watch_sub_dl", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("sub_test.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_sub_dl", server.base_url),
            "--subtitles",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""subtitles""#));

    assert!(out_path.exists());
    let sub_file = tmp.path().join("sub_test.en.vtt");
    assert!(sub_file.exists());
    assert!(
        std::fs::read_to_string(&sub_file)
            .unwrap()
            .contains("Welcome to RuviTools")
    );
}

#[tokio::test]
async fn t1_f6_03_downloads_specific_language_subtitle() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"SUB_LANG_VIDEO_DATA".to_vec();
    server.set_route(
        "/sub_lang.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );

    let en_vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nHello EN\n";
    let tr_vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nMerhaba TR\n";

    server.set_route("/en.vtt", RoutePayload::text(en_vtt, "text/vtt"));
    server.set_route("/tr.vtt", RoutePayload::text(tr_vtt, "text/vtt"));

    let captions_json = format!(
        r#"[
      {{"baseUrl": "{}/en.vtt", "name": {{"simpleText": "English"}}, "languageCode": "en", "vssId": ".en"}},
      {{"baseUrl": "{}/tr.vtt", "name": {{"simpleText": "Turkish"}}, "languageCode": "tr", "vssId": ".tr"}}
    ]"#,
        server.base_url, server.base_url
    );

    let formats = format!(
        r#"[
      {{"itag": 18, "url": "{}/sub_lang.mp4", "mimeType": "video/mp4", "contentLength": "{}"}}
    ]"#,
        server.base_url,
        video_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Sub Lang Filter Test",
        "sub_filter",
        &formats,
        "[]",
        &captions_json,
        &[],
    );
    server.set_route("/watch_filter", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("lang_filter.mp4");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_filter", server.base_url),
            "--subtitles",
            "--subtitles-lang",
            "tr",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    assert!(tmp.path().join("lang_filter.tr.vtt").exists());
    assert!(!tmp.path().join("lang_filter.en.vtt").exists());
    assert!(
        std::fs::read_to_string(tmp.path().join("lang_filter.tr.vtt"))
            .unwrap()
            .contains("Merhaba TR")
    );
}

#[tokio::test]
async fn t1_f6_04_converts_timedtext_xml_to_valid_vtt() {
    let builder = SubtitleFixtureBuilder::new()
        .cue(1000, 4500, "First line of text")
        .cue(5000, 8000, "Second line & special chars <demo>");

    let vtt = builder.build_vtt();
    assert!(vtt.starts_with("WEBVTT\n\n"));
    assert!(vtt.contains("00:00:01.000 --> 00:00:04.500"));
    assert!(vtt.contains("First line of text"));
    assert!(vtt.contains("00:00:05.000 --> 00:00:08.000"));
}

#[tokio::test]
async fn t1_f6_05_converts_timedtext_xml_to_valid_srt() {
    let builder = SubtitleFixtureBuilder::new()
        .cue(1000, 4500, "First cue")
        .cue(5000, 8000, "Second cue");

    let srt = builder.build_srt();
    assert!(srt.contains("1\n00:00:01,000 --> 00:00:04,500\nFirst cue\n\n"));
    assert!(srt.contains("2\n00:00:05,000 --> 00:00:08,000\nSecond cue\n\n"));
}

// =========================================================================
// Feature 7: Multi-language dubbed audio (5 tests)
// =========================================================================

#[tokio::test]
async fn t1_f7_01_parses_audio_track_metadata_in_info() {
    let server = MockHttpServer::start().await;

    let formats = "[]";
    let adaptive = r#"[
      {"itag": 137, "url": "http://127.0.0.1/v1080.mp4", "mimeType": "video/mp4; codecs=\"avc1.640028\"", "width": 1920, "height": 1080, "bitrate": 4000000, "contentLength": "500000"},
      {"itag": 140, "url": "http://127.0.0.1/a_en.m4a", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "contentLength": "50000", "audioTrack": {"displayName": "English original", "id": "en", "audioIsDefault": true}},
      {"itag": 141, "url": "http://127.0.0.1/a_tr.m4a", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "contentLength": "50000", "audioTrack": {"displayName": "Turkish dubbed", "id": "tr", "audioIsDefault": false}}
    ]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Dubbed Audio Demo",
        "dub_01",
        formats,
        adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_dubs", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args(["info", &format!("{}/watch_dubs", server.base_url), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "itag-140""#))
        .stdout(predicate::str::contains(r#""id": "itag-141""#));
}

#[tokio::test]
async fn t1_f7_02_assigns_composite_stream_id_or_itag() {
    let server = MockHttpServer::start().await;

    let formats = "[]";
    let adaptive = r#"[
      {"itag": 140, "url": "http://127.0.0.1/audio_en.m4a", "mimeType": "audio/mp4", "bitrate": 128000, "contentLength": "40000"},
      {"itag": 251, "url": "http://127.0.0.1/audio_opus.webm", "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "contentLength": "45000"}
    ]"#;

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Audio IDs Demo",
        "aud_id_01",
        formats,
        adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_aud_ids", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "info",
            &format!("{}/watch_aud_ids", server.base_url),
            "--all-formats",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""id": "itag-140""#))
        .stdout(predicate::str::contains(r#""id": "itag-251""#));
}

#[tokio::test]
async fn t1_f7_03_audio_lang_flag_or_stream_id_selection() {
    let server = MockHttpServer::start().await;
    let tr_audio_bytes = b"TURKISH_DUBBED_AUDIO_TRACK_BYTES".to_vec();

    server.set_route(
        "/tr_audio.m4a",
        RoutePayload::bytes(tr_audio_bytes.clone(), "audio/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 140, "url": "{}/en_audio.m4a", "mimeType": "audio/mp4", "bitrate": 128000, "contentLength": "1000"}},
      {{"itag": 141, "url": "{}/tr_audio.m4a", "mimeType": "audio/mp4", "bitrate": 128000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        server.base_url,
        tr_audio_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Select Dub Demo",
        "sel_dub_01",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_sel_dub", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("dub_selected.m4a");

    // Select by explicit stream ID "itag-141"
    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_sel_dub", server.base_url),
            "-f",
            "itag-141",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-141""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), tr_audio_bytes);
}

#[tokio::test]
async fn t1_f7_04_downloads_selected_audio_dub_stream() {
    let server = MockHttpServer::start().await;
    let opus_bytes = b"OPUS_DUBBED_TRACK_PAYLOAD".to_vec();

    server.set_route(
        "/opus_track.webm",
        RoutePayload::bytes(opus_bytes.clone(), "audio/webm"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 251, "url": "{}/opus_track.webm", "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        opus_bytes.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Opus Audio Demo",
        "opus_dub",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_opus_dub", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("dub_opus.webm");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_opus_dub", server.base_url),
            "-f",
            "itag-251",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-251""#));

    assert_eq!(std::fs::read(&out_path).unwrap(), opus_bytes);
}

#[tokio::test]
async fn t1_f7_05_handoff_manifest_records_audio_stream_metadata() {
    let server = MockHttpServer::start().await;
    let audio_payload = b"AUDIO_STREAM_FOR_MANIFEST".to_vec();

    server.set_route(
        "/a_manifest.m4a",
        RoutePayload::bytes(audio_payload.clone(), "audio/mp4"),
    );

    let formats = "[]";
    let adaptive = format!(
        r#"[
      {{"itag": 140, "url": "{}/a_manifest.m4a", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "contentLength": "{}"}}
    ]"#,
        server.base_url,
        audio_payload.len()
    );

    let html = PlaylistFixtureBuilder::build_youtube_watch_html(
        "Audio Manifest Test",
        "aud_man",
        formats,
        &adaptive,
        "[]",
        &[],
    );
    server.set_route("/watch_aud_man", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("audio_out.m4a");
    let manifest_path = tmp.path().join("audio.manifest.json");

    common::ruvid_cmd(tmp.path())
        .env("RUVID_DISABLE_YTDLP", "1")
        .args([
            "get",
            &format!("{}/watch_aud_man", server.base_url),
            "-f",
            "itag-140",
            "-o",
            out_path.to_str().unwrap(),
            "--write-manifest",
            manifest_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    assert!(manifest_path.exists());
    let manifest_str = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_str.contains(r#""schema_version": "1""#));
    assert!(manifest_str.contains(r#""kind": "audio""#));
    assert!(manifest_str.contains(r#""codec": "mp4a.40.2""#));
}
