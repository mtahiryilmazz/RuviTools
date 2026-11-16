mod common;

use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;

#[tokio::test]
async fn hls_vod_variant_selection_ordered_join_subtitles_and_drm_rejection() {
    let server = MockHttpServer::start().await;

    let master_m3u8 = include_str!("fixtures/hls/master.m3u8");
    let media_720_m3u8 = include_str!("fixtures/hls/media_720p.m3u8");
    let drm_m3u8 = include_str!("fixtures/hls/drm.m3u8");

    server.set_route(
        "/hls/master.m3u8",
        RoutePayload::text(master_m3u8, "application/vnd.apple.mpegurl"),
    );
    server.set_route(
        "/hls/720p/playlist.m3u8",
        RoutePayload::text(media_720_m3u8, "application/vnd.apple.mpegurl"),
    );
    server.set_route(
        "/hls/720p/seg0.ts",
        RoutePayload::bytes(b"SEG0-".to_vec(), "video/mp2t"),
    );
    server.set_route(
        "/hls/720p/seg1.ts",
        RoutePayload::bytes(b"SEG1-".to_vec(), "video/mp2t"),
    );
    server.set_route(
        "/hls/720p/seg2.ts",
        RoutePayload::bytes(b"SEG2".to_vec(), "video/mp2t"),
    );
    server.set_route(
        "/hls/subs/en.vtt",
        RoutePayload::text(
            "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nHello\n",
            "text/vtt",
        ),
    );
    server.set_route(
        "/hls/drm.m3u8",
        RoutePayload::text(drm_m3u8, "application/vnd.apple.mpegurl"),
    );

    let tmp = tempfile::tempdir().unwrap();

    // 1. Inspect master playlist with `info`
    common::ruvid_cmd(tmp.path())
        .args([
            "info",
            &format!("{}/hls/master.m3u8", server.base_url),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("hls-720p"))
        .stdout(predicate::str::contains("hls-1080p"));

    // 2. Download 720p variant + subtitles; verify exact ordered assembly
    let hls_out = tmp.path().join("hls_720p.ts");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/hls/master.m3u8", server.base_url),
            "-f",
            "720p",
            "--jobs",
            "3",
            "--subtitles",
            "--subtitles-lang",
            "en",
            "-o",
            hls_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""chunks_total": 3"#));

    assert_eq!(std::fs::read(&hls_out).unwrap(), b"SEG0-SEG1-SEG2");
    let sub_out = tmp.path().join("hls_720p.en.vtt");
    assert!(sub_out.exists());
    assert!(std::fs::read_to_string(sub_out).unwrap().contains("WEBVTT"));

    // 3. Reject DRM playlist with exit code 4
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/hls/drm.m3u8", server.base_url),
            "-o",
            tmp.path().join("drm.ts").to_str().unwrap(),
        ])
        .assert()
        .code(4);
}

#[tokio::test]
async fn dash_vod_segment_template_assembly_and_drm_rejection() {
    let server = MockHttpServer::start().await;

    let mpd_xml = include_str!("fixtures/dash/vod.mpd");
    let drm_mpd_xml = include_str!("fixtures/dash/drm.mpd");

    server.set_route(
        "/dash/vod.mpd",
        RoutePayload::text(mpd_xml, "application/dash+xml"),
    );
    server.set_route(
        "/dash/v720/init.mp4",
        RoutePayload::bytes(b"INIT:".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/dash/v720/seg-001.m4s",
        RoutePayload::bytes(b"M1:".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/dash/v720/seg-002.m4s",
        RoutePayload::bytes(b"M2".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/dash/drm.mpd",
        RoutePayload::text(drm_mpd_xml, "application/dash+xml"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let dash_out = tmp.path().join("dash.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/dash/vod.mpd", server.base_url),
            "-f",
            "v720",
            "--jobs",
            "2",
            "-o",
            dash_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "v720""#));

    assert_eq!(std::fs::read(&dash_out).unwrap(), b"INIT:M1:M2");

    // Reject DRM ContentProtection MPD with exit code 4
    common::ruvid_cmd(tmp.path())
        .args(["info", &format!("{}/dash/drm.mpd", server.base_url)])
        .assert()
        .code(4);
}

#[tokio::test]
async fn playlist_selection_and_skip_errors_work() {
    let server = MockHttpServer::start().await;
    server.set_route(
        "/catalog.m3u",
        RoutePayload::text(
            "#EXTM3U\n#EXTINF:10,First Clip\n/c1.mp4\n#EXTINF:10,Broken Clip\n/missing.mp4\n#EXTINF:10,Third Clip\n/c3.mp4\n",
            "audio/x-mpegurl",
        ),
    );
    server.set_route(
        "/c1.mp4",
        RoutePayload::bytes(b"CLIP1".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/c3.mp4",
        RoutePayload::bytes(b"CLIP3".to_vec(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let pl_out = tmp.path().join("playlist_out");

    common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            &format!("{}/catalog.m3u", server.base_url),
            "--items",
            "1-3",
            "--concurrency",
            "2",
            "--skip-errors",
            "-o",
            pl_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""succeeded": 2"#))
        .stdout(predicate::str::contains(r#""failed": 1"#));
}
