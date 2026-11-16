mod common;

use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;

#[tokio::test]
async fn direct_http_download_retry_overwrite_and_manifest_redaction() {
    let server = MockHttpServer::start().await;
    let payload_bytes = b"DIRECT_VIDEO_PAYLOAD_BYTES_1234567890".to_vec();

    server.set_route(
        "/video.mp4",
        RoutePayload::bytes(payload_bytes.clone(), "video/mp4"),
    );

    // Transient 429 then 200 OK
    let mut err_429 = RoutePayload::text("rate limited", "text/plain");
    err_429.status = 429;
    server.set_sequence(
        "/flaky.mp4",
        vec![
            err_429,
            RoutePayload::bytes(payload_bytes.clone(), "video/mp4"),
        ],
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_file = tmp.path().join("downloaded.mp4");
    let manifest_file = tmp.path().join("handoff.ruvi.json");

    // 1. Download with secret token query param + write manifest -> secret token redacted
    let url_with_token = format!(
        "{}/video.mp4?token=TOP_SECRET_TOKEN&view=1",
        server.base_url
    );
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &url_with_token,
            "-o",
            out_file.to_str().unwrap(),
            "--write-manifest",
            manifest_file.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains("TOP_SECRET_TOKEN").not());

    assert_eq!(std::fs::read(&out_file).unwrap(), payload_bytes);
    let manifest_text = std::fs::read_to_string(&manifest_file).unwrap();
    assert!(manifest_text.contains(r#""schema_version": "1""#));
    assert!(manifest_text.contains("REDACTED"));
    assert!(!manifest_text.contains("TOP_SECRET_TOKEN"));

    // 2. Overwrite protection rejects existing destination without --overwrite
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/video.mp4", server.base_url),
            "-o",
            out_file.to_str().unwrap(),
        ])
        .assert()
        .code(5);

    // 3. Transient 429 recovers via retry
    let flaky_out = tmp.path().join("flaky.mp4");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/flaky.mp4", server.base_url),
            "--retry",
            "3",
            "-o",
            flaky_out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(std::fs::read(&flaky_out).unwrap(), payload_bytes);

    // 4. 404 fails cleanly with Network exit code 3
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/missing.mp4", server.base_url),
            "-o",
            tmp.path().join("missing.mp4").to_str().unwrap(),
        ])
        .assert()
        .code(3);
}

#[tokio::test]
async fn template_sanitizes_unsafe_filename_and_platform_html_resolves_og_video() {
    let server = MockHttpServer::start().await;
    let media_bytes = b"OG_VIDEO_MEDIA_CONTENT".to_vec();
    server.set_route(
        "/media/clip.mp4",
        RoutePayload::bytes(media_bytes.clone(), "video/mp4"),
    );

    let html = format!(
        r#"<html><head>
        <title>CON..Evil:Title/Test?</title>
        <meta property="og:video" content="{}/media/clip.mp4">
        </head><body></body></html>"#,
        server.base_url
    );
    server.set_route("/watch", RoutePayload::text(&html, "text/html"));

    let tmp = tempfile::tempdir().unwrap();
    let tpl = tmp.path().join("%(title)s.%(ext)s");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/watch", server.base_url),
            "-o",
            tpl.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#));

    let files: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("mp4"))
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(std::fs::read(files[0].path()).unwrap(), media_bytes);
}
