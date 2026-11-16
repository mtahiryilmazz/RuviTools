mod common;

use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;

#[tokio::test]
async fn parallel_range_download_and_single_stream_fallback_work() {
    let server = MockHttpServer::start().await;
    let total_size = 48 * 1024;
    let payload: Vec<u8> = (0..total_size).map(|i| (i % 251) as u8).collect();

    server.set_route(
        "/large.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    // Server claims Accept-Ranges: none and ignores Range (always returns 200)
    let mut no_range = RoutePayload::bytes(payload.clone(), "video/mp4");
    no_range.accept_ranges = false;
    no_range.honor_range = false;
    server.set_route("/norange.mp4", no_range);

    let tmp = tempfile::tempdir().unwrap();
    let parallel_out = tmp.path().join("parallel.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/large.mp4", server.base_url),
            "--jobs",
            "4",
            "-o",
            parallel_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""bytes_downloaded": 49152"#));
    assert_eq!(std::fs::read(&parallel_out).unwrap(), payload);

    // Verify multiple 206 byte range requests occurred
    let logs = server.request_log();
    let range_reqs = logs
        .iter()
        .filter(|l| l.starts_with("/large.mp4") && l.contains("range=bytes="))
        .count();
    assert!(
        range_reqs >= 4,
        "expected parallel range requests, got: {logs:?}"
    );

    // Fallback when range is not supported
    let fallback_out = tmp.path().join("fallback.mp4");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/norange.mp4", server.base_url),
            "--jobs",
            "4",
            "-o",
            fallback_out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(std::fs::read(&fallback_out).unwrap(), payload);
}

#[tokio::test]
async fn resume_from_checkpoint_downloads_only_missing_chunks_and_rejects_etag_change() {
    let server = MockHttpServer::start().await;
    let total_size = 32 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| ((i * 7) % 253) as u8).collect();
    let url = format!("{}/resumable.mp4", server.base_url);

    server.set_route(
        "/resumable.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("resumable.mp4");
    let part_path = tmp.path().join("resumable.mp4.part");
    let ckpt_path = tmp.path().join("resumable.mp4.part.ruvid.json");

    // Pre-populate .part with first half (chunks 0..4 of 8) and valid matching checkpoint
    let chunks = ruvid::downloader::http_range::plan_chunks(total_size as u64, 4);
    assert_eq!(chunks.len(), 8);
    let mut part_buf = vec![0u8; total_size];
    let split = chunks[4].start as usize;
    part_buf[..split].copy_from_slice(&payload[..split]);
    std::fs::write(&part_path, &part_buf).unwrap();

    let mut ckpt = ruvid::storage::DownloadCheckpoint::new(
        &url,
        "direct-0",
        Some("\"v1\"".to_string()),
        Some("Mon, 16 Nov 2026 00:00:00 GMT".to_string()),
        Some(total_size as u64),
        chunks[0].len(),
        chunks.len(),
    );
    for idx in 0..4 {
        ckpt.completed_chunks.insert(idx);
    }
    ckpt.save_atomic(&ckpt_path).unwrap();

    // Resume should fetch only remaining 4 chunks (plus 1 initial probe)
    server.clear_requests();
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &url,
            "--jobs",
            "4",
            "--resume",
            "-o",
            out_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""resumed": true"#))
        .stdout(predicate::str::contains(r#""chunks_resumed": 4"#));
    assert_eq!(std::fs::read(&out_path).unwrap(), payload);
    assert!(!ckpt_path.exists());

    // Now test ETag mismatch rejection when resuming stale checkpoint
    let stale_out = tmp.path().join("stale.mp4");
    let stale_part = tmp.path().join("stale.mp4.part");
    let stale_ckpt_path = tmp.path().join("stale.mp4.part.ruvid.json");
    std::fs::write(&stale_part, vec![0u8; total_size]).unwrap();

    let mut changed_route = RoutePayload::bytes(payload, "video/mp4");
    changed_route.etag = Some("\"v2-changed\"".to_string());
    server.set_route("/changed.mp4", changed_route);
    let changed_url = format!("{}/changed.mp4", server.base_url);

    let mut stale_ckpt = ruvid::storage::DownloadCheckpoint::new(
        &changed_url,
        "direct-0",
        Some("\"v1-old\"".to_string()),
        Some("Mon, 16 Nov 2026 00:00:00 GMT".to_string()),
        Some(total_size as u64),
        chunks[0].len(),
        chunks.len(),
    );
    stale_ckpt.completed_chunks.insert(0);
    stale_ckpt.save_atomic(&stale_ckpt_path).unwrap();

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &changed_url,
            "--jobs",
            "4",
            "--resume",
            "-o",
            stale_out.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!stale_out.exists());
}
