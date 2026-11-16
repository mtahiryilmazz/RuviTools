mod common;

use std::sync::{Arc, Mutex};

use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use ruvid::domain::media_manifest::redact_sensitive_url;
use ruvid::resolver::validate_url_security;
use ruvid::storage::{
    DownloadCheckpoint, commit_part_file, resolve_output_path, sanitize_filename,
};

#[derive(Clone)]
struct RedirectProxyTarget {
    location: String,
    html_body: Option<String>,
}

/// Minimal local HTTP proxy server used strictly in tests to verify that `ruvid`
/// blocks HTTP 302 redirects and HTML/manifest references targeting private/loopback
/// networks without needing external internet access.
struct RedirectTestProxy {
    proxy_url: String,
    requested_uris: Arc<Mutex<Vec<String>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RedirectTestProxy {
    async fn start(target: RedirectProxyTarget) -> Self {
        let requested_uris: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let reqs_c = requested_uris.clone();
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (tx, mut rx) = tokio::sync::oneshot::channel();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build redirect proxy runtime");
            rt.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind redirect test proxy");
                let addr = listener.local_addr().unwrap();
                let _ = addr_tx.send(addr);

                loop {
                    tokio::select! {
                        _ = &mut rx => break,
                        accepted = listener.accept() => {
                            let Ok((mut stream, _)) = accepted else {
                                break;
                            };
                            let reqs_h = reqs_c.clone();
                            let target_h = target.clone();
                            tokio::spawn(async move {
                                let mut buf = [0u8; 4096];
                                let n = match stream.read(&mut buf).await {
                                    Ok(n) if n > 0 => n,
                                    _ => return,
                                };
                                let req_str = String::from_utf8_lossy(&buf[..n]);
                                let uri = req_str
                                    .lines()
                                    .next()
                                    .and_then(|l| l.split_whitespace().nth(1))
                                    .unwrap_or("")
                                    .to_string();
                                reqs_h.lock().unwrap().push(uri);

                                if let Some(ref html) = target_h.html_body {
                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        html.len(),
                                        html
                                    );
                                    let _ = stream.write_all(resp.as_bytes()).await;
                                } else {
                                    let resp = format!(
                                        "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                        target_h.location
                                    );
                                    let _ = stream.write_all(resp.as_bytes()).await;
                                }
                            });
                        }
                    }
                }
            });
        });

        let addr = addr_rx.recv().expect("receive proxy addr");
        Self {
            proxy_url: format!("http://{addr}"),
            requested_uris,
            shutdown: Some(tx),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requested_uris.lock().unwrap().clone()
    }
}

impl Drop for RedirectTestProxy {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

#[test]
fn ssrf_blocks_loopback_private_link_local_cgnat_ipv6_and_localhost_variants() {
    let blocked_urls = [
        "http://127.0.0.1/video.mp4",
        "http://127.0.1.1:8080/video.mp4",
        "http://0.0.0.0/video.mp4",
        "http://0.1.2.3/video.mp4",
        "http://10.0.0.1/video.mp4",
        "http://10.255.255.254/video.mp4",
        "http://172.16.0.1/video.mp4",
        "http://172.31.255.254/video.mp4",
        "http://192.168.0.1/video.mp4",
        "http://192.168.1.254/video.mp4",
        "http://169.254.169.254/latest/meta-data/",
        "http://169.254.0.1/video.mp4",
        "http://100.64.0.1/video.mp4",
        "http://100.127.255.254/video.mp4",
        "http://[::1]:8080/video.mp4",
        "http://[::]/video.mp4",
        "http://[fe80::1]/video.mp4",
        "http://[fc00::1]/video.mp4",
        "http://[fd12:3456::1]/video.mp4",
        "http://[::ffff:127.0.0.1]/video.mp4",
        "http://[::ffff:169.254.169.254]/latest/meta-data/",
        "http://[::ffff:10.0.0.1]/video.mp4",
        "http://[::ffff:100.64.0.1]/video.mp4",
        "http://[::127.0.0.1]/video.mp4",
        "http://[64:ff9b::127.0.0.1]/video.mp4",
        "http://localhost/video.mp4",
        "http://localhost./video.mp4",
        "http://sub.localhost/video.mp4",
        "http://evil.sub.localhost./video.mp4",
        "http://printer.local/video.mp4",
        "http://printer.local./video.mp4",
        "ftp://example.com/video.mp4",
        "file:///etc/passwd",
    ];

    for raw in blocked_urls {
        let parsed = Url::parse(raw).expect("valid test URL");
        assert!(
            validate_url_security(&parsed, false).is_err(),
            "expected SSRF rejection for {raw}"
        );
    }

    // Verify public IP ranges (including boundaries outside CGNAT and 172.16/12) are allowed
    let allowed_urls = [
        "https://93.184.216.34/video.mp4",
        "https://100.63.255.254/video.mp4",
        "https://100.128.0.1/video.mp4",
        "https://172.15.255.254/video.mp4",
        "https://172.32.0.1/video.mp4",
        "https://example.com/video.mp4",
    ];
    for raw in allowed_urls {
        let parsed = Url::parse(raw).expect("valid public URL");
        assert!(
            validate_url_security(&parsed, false).is_ok(),
            "expected public URL to pass SSRF validation: {raw}"
        );
    }

    // CLI enforcement check for representative private/loopback/localhost targets without RUVID_ALLOW_PRIVATE_NET
    let tmp = tempfile::tempdir().unwrap();
    for cli_blocked in [
        "http://127.0.0.1:9999/v.mp4",
        "http://10.1.2.3/v.mp4",
        "http://172.16.5.4/v.mp4",
        "http://192.168.1.10/v.mp4",
        "http://169.254.169.254/latest/meta-data/",
        "http://100.64.1.1/v.mp4",
        "http://[::1]:8080/v.mp4",
        "http://[::ffff:127.0.0.1]/v.mp4",
        "http://sub.localhost/v.mp4",
        "http://sub.localhost./v.mp4",
    ] {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("ruvid");
        cmd.env("RUVI_CONFIG_DIR", tmp.path().join("cfg"))
            .env_remove("RUVID_ALLOW_PRIVATE_NET")
            .args(["info", cli_blocked])
            .assert()
            .code(4);
    }
}

#[tokio::test]
async fn ssrf_blocks_http_redirects_and_html_refs_to_private_hosts() {
    let tmp = tempfile::tempdir().unwrap();

    for target in [
        "http://127.0.0.1:8080/admin?token=SECRET_REDIRECT_TOK",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.5/internal.mp4",
        "http://172.16.0.9/internal.mp4",
        "http://192.168.1.1/internal.mp4",
        "http://100.64.0.5/internal.mp4",
        "http://[::1]/internal.mp4",
        "http://api.localhost/internal.mp4",
    ] {
        let proxy = RedirectTestProxy::start(RedirectProxyTarget {
            location: target.to_string(),
            html_body: None,
        })
        .await;

        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("ruvid");
        cmd.env("RUVI_CONFIG_DIR", tmp.path().join("cfg"))
            .env_remove("RUVID_ALLOW_PRIVATE_NET")
            .args([
                "get",
                "http://public.example.test/start.mp4",
                "--proxy",
                &proxy.proxy_url,
                "-o",
                tmp.path().join("out.mp4").to_str().unwrap(),
            ])
            .assert()
            .code(4)
            .stderr(predicate::str::contains("SECRET_REDIRECT_TOK").not());

        let seen = proxy.requests();
        assert_eq!(
            seen.len(),
            1,
            "proxy must only see initial request, got {seen:?}"
        );
        assert!(
            seen[0].contains("public.example.test"),
            "redirect target {target} should never have been requested: {seen:?}"
        );
    }

    // HTML og:video pointing to private metadata endpoint must also be blocked with exit code 4
    let html_proxy = RedirectTestProxy::start(RedirectProxyTarget {
        location: String::new(),
        html_body: Some(
            r#"<html><head><meta property="og:video" content="http://169.254.169.254/latest/meta-data/"></head></html>"#
                .to_string(),
        ),
    })
    .await;

    let cfg_path = tmp.path().join("proxy_info.toml");
    std::fs::write(
        &cfg_path,
        format!("[network]\nproxy = \"{}\"\n", html_proxy.proxy_url),
    )
    .unwrap();

    let mut cmd_html = assert_cmd::cargo::cargo_bin_cmd!("ruvid");
    cmd_html
        .env("RUVI_CONFIG_DIR", tmp.path().join("cfg"))
        .env_remove("RUVID_ALLOW_PRIVATE_NET")
        .args([
            "--config",
            cfg_path.to_str().unwrap(),
            "info",
            "http://public.example.test/watch",
        ])
        .assert()
        .code(4);
    let html_reqs = html_proxy.requests();
    assert_eq!(
        html_reqs.len(),
        2,
        "expected probe + HTML fetch only, got {html_reqs:?}"
    );
    assert!(
        html_reqs
            .iter()
            .all(|u| u.contains("public.example.test/watch")),
        "private target must never be requested through proxy: {html_reqs:?}"
    );
}

#[test]
fn filename_sanitization_blocks_path_traversal_reserved_names_control_chars_and_trailing_dots() {
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    for name in reserved {
        let s_bare = sanitize_filename(name);
        assert!(
            s_bare.starts_with('_'),
            "expected prefix for reserved name {name}, got {s_bare}"
        );
        let s_lower_ext = sanitize_filename(&format!("{}.mp4", name.to_ascii_lowercase()));
        assert!(
            s_lower_ext.starts_with('_'),
            "expected prefix for lowercase reserved name {name}.mp4, got {s_lower_ext}"
        );
        let s_spaced_ext = sanitize_filename(&format!("{name} .mp4"));
        assert!(
            s_spaced_ext.starts_with('_'),
            "expected prefix for spaced reserved name '{name} .mp4', got {s_spaced_ext}"
        );
        let s_dot = sanitize_filename(&format!("{name}."));
        assert!(
            s_dot.starts_with('_'),
            "expected prefix for trailing-dot reserved name '{name}.', got {s_dot}"
        );
    }

    let traversal = sanitize_filename("../../..\\Windows\\System32\\evil");
    assert!(!traversal.contains(".."));
    assert!(!traversal.contains('/'));
    assert!(!traversal.contains('\\'));

    let ctrl = sanitize_filename("bad\x00name\x1fwith\x7fctrl");
    assert!(!ctrl.chars().any(|c| c.is_control()));
    assert_eq!(ctrl, "bad_name_with_ctrl");

    let dots = sanitize_filename("video_title...");
    assert!(!dots.ends_with('.'));
    assert!(!dots.contains(".."));

    assert_eq!(sanitize_filename("..."), "_");
    assert_eq!(sanitize_filename("   ...  "), "_");
    assert_eq!(sanitize_filename("   .  "), "media");

    // Template with `..` traversal component must be rejected cleanly
    let tmp = tempfile::tempdir().unwrap();
    let bad_tpl = tmp.path().join("../escape/%(title)s.%(ext)s");
    let res = resolve_output_path(Some(&bad_tpl), tmp.path(), "Title", "mp4", "id0");
    assert!(res.is_err());

    // Malicious title inside `%(title)s.%(ext)s` must remain inside target directory
    let safe_tpl = tmp.path().join("%(title)s.%(ext)s");
    let safe_out = resolve_output_path(
        Some(&safe_tpl),
        tmp.path(),
        "../../../CON\x07...",
        "mp4",
        "id0",
    )
    .unwrap();
    assert_eq!(safe_out.parent().unwrap(), tmp.path());
    let file_name = safe_out.file_name().unwrap().to_str().unwrap();
    assert!(!file_name.contains(".."));
    assert!(!file_name.chars().any(|c| c.is_control()));
}

#[tokio::test]
async fn redacts_credentials_and_tokens_across_manifests_json_output_and_error_logs() {
    let secret_url_parts = "https://alice:SUPER_PASS_99@media.example.invalid/video.mp4\
        ?token=TOK_111&sig=SIG_222&key=KEY_333&auth=AUTH_444&secret=SEC_555&hdnea=HDNEA_666&policy=POL_777&ok=1";
    let redacted = redact_sensitive_url(secret_url_parts);
    assert!(redacted.contains("REDACTED:REDACTED@"));
    assert!(redacted.contains("ok=1"));
    for leak in [
        "alice",
        "SUPER_PASS_99",
        "TOK_111",
        "SIG_222",
        "KEY_333",
        "AUTH_444",
        "SEC_555",
        "HDNEA_666",
        "POL_777",
    ] {
        assert!(
            !redacted.contains(leak),
            "expected '{leak}' to be redacted in {redacted}"
        );
    }

    let server = MockHttpServer::start().await;
    let payload = b"SECRET_MEDIA_BYTES_FOR_REDACTION_TEST".to_vec();
    server.set_route("/protected.mp4", RoutePayload::bytes(payload, "video/mp4"));

    let mut err_403 = RoutePayload::text("forbidden", "text/plain");
    err_403.status = 403;
    server.set_route("/denied.mp4", err_403);

    let base_no_scheme = server.base_url.strip_prefix("http://").unwrap();
    let authed_ok_url = format!(
        "http://myuser:MY_SECRET_PASS@{base_no_scheme}/protected.mp4\
         ?token=T1&sig=S2&key=K3&auth=A4&secret=X5&hdnea=H6&policy=P7&view=public"
    );
    let authed_err_url = format!(
        "http://myuser:MY_SECRET_PASS@{base_no_scheme}/denied.mp4\
         ?token=T1&sig=S2&key=K3&auth=A4&secret=X5&hdnea=H6&policy=P7"
    );

    let tmp = tempfile::tempdir().unwrap();

    // 1. `ruvid info --json` must redact all stream & top-level URLs
    let info_out = common::ruvid_cmd(tmp.path())
        .args(["info", &authed_ok_url, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let info_text = String::from_utf8(info_out).unwrap();
    assert!(info_text.contains("REDACTED"));
    assert!(info_text.contains("view=public"));
    for leak in [
        "myuser",
        "MY_SECRET_PASS",
        "T1",
        "S2",
        "K3",
        "A4",
        "X5",
        "H6",
        "P7",
    ] {
        assert!(
            !info_text.contains(leak),
            "`ruvid info --json` leaked '{leak}': {info_text}"
        );
    }

    // 2. `ruvid get --json --write-manifest` must redact JSON output, manifest, and -vv stderr logs
    let out_file = tmp.path().join("redacted.mp4");
    let manifest_file = tmp.path().join("redacted.ruvi.json");
    let get_assert = common::ruvid_cmd(tmp.path())
        .args([
            "-vv",
            "get",
            &authed_ok_url,
            "-o",
            out_file.to_str().unwrap(),
            "--write-manifest",
            manifest_file.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    let get_out = get_assert.get_output();
    let stdout_str = String::from_utf8_lossy(&get_out.stdout);
    let stderr_str = String::from_utf8_lossy(&get_out.stderr);
    let manifest_str = std::fs::read_to_string(&manifest_file).unwrap();

    for leak in [
        "myuser",
        "MY_SECRET_PASS",
        "T1",
        "S2",
        "K3",
        "A4",
        "X5",
        "H6",
        "P7",
    ] {
        assert!(
            !stdout_str.contains(leak),
            "`ruvid get --json` stdout leaked '{leak}': {stdout_str}"
        );
        assert!(
            !stderr_str.contains(leak),
            "`ruvid get -vv` stderr leaked '{leak}': {stderr_str}"
        );
        assert!(
            !manifest_str.contains(leak),
            "manifest leaked '{leak}': {manifest_str}"
        );
    }

    // 3. Error logs & JSON error output must redact credentials & tokens
    let err_assert = common::ruvid_cmd(tmp.path())
        .args(["-vv", "info", &authed_err_url])
        .assert()
        .code(3);
    let err_stderr = String::from_utf8_lossy(&err_assert.get_output().stderr);
    for leak in [
        "myuser",
        "MY_SECRET_PASS",
        "T1",
        "S2",
        "K3",
        "A4",
        "X5",
        "H6",
        "P7",
    ] {
        assert!(
            !err_stderr.contains(leak),
            "error stderr leaked '{leak}': {err_stderr}"
        );
    }

    let err_json_assert = common::ruvid_cmd(tmp.path())
        .args(["info", &authed_err_url, "--json"])
        .assert()
        .code(3);
    let err_stdout = String::from_utf8_lossy(&err_json_assert.get_output().stdout);
    for leak in [
        "myuser",
        "MY_SECRET_PASS",
        "T1",
        "S2",
        "K3",
        "A4",
        "X5",
        "H6",
        "P7",
    ] {
        assert!(
            !err_stdout.contains(leak),
            "JSON error leaked '{leak}': {err_stdout}"
        );
    }
}

#[tokio::test]
async fn rejects_stdout_combined_with_write_manifest_dash_with_exit_code_2() {
    let server = MockHttpServer::start().await;
    let body = b"STDOUT_STREAM_DATA".to_vec();
    server.set_route(
        "/stream.mp4",
        RoutePayload::bytes(body.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/stream.mp4", server.base_url),
            "--stdout",
            "--write-manifest",
            "-",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--stdout"));

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/stream.mp4", server.base_url),
            "--stdout",
            "--write-manifest",
            "-",
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(r#""exit_code": 2"#));

    // `--write-manifest -` alone is also invalid (exit code 2)
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/stream.mp4", server.base_url),
            "-o",
            tmp.path().join("x.mp4").to_str().unwrap(),
            "--write-manifest",
            "-",
        ])
        .assert()
        .code(2);

    // Confirm `--stdout` with a real file `--write-manifest <file>` works cleanly without mixing streams
    let mf = tmp.path().join("stdout_job.ruvi.json");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/stream.mp4", server.base_url),
            "--stdout",
            "--write-manifest",
            mf.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::eq("STDOUT_STREAM_DATA"));
    assert!(mf.exists());
}

#[tokio::test]
async fn rejects_corrupt_checkpoints_and_byte_count_mismatches_with_exit_code_8() {
    let server = MockHttpServer::start().await;
    let total_size = 32 * 1024usize;
    let payload: Vec<u8> = (0..total_size).map(|i| (i % 241) as u8).collect();
    let url = format!("{}/range.mp4", server.base_url);
    server.set_route(
        "/range.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();

    // 1. Corrupt JSON checkpoint file
    let out1 = tmp.path().join("corrupt_json.mp4");
    let part1 = tmp.path().join("corrupt_json.mp4.part");
    let ckpt1 = tmp.path().join("corrupt_json.mp4.part.ruvid.json");
    std::fs::write(&part1, vec![0u8; total_size]).unwrap();
    std::fs::write(&ckpt1, "{ NOT VALID JSON !!!").unwrap();

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &url,
            "--jobs",
            "4",
            "--resume",
            "-o",
            out1.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!out1.exists());

    // 2. Unsupported version / chunk layout mismatch (e.g. checkpoint from --jobs 4 resumed with --jobs 2)
    let out2 = tmp.path().join("layout_mismatch.mp4");
    let part2 = tmp.path().join("layout_mismatch.mp4.part");
    let ckpt2 = tmp.path().join("layout_mismatch.mp4.part.ruvid.json");
    std::fs::write(&part2, vec![0u8; total_size]).unwrap();
    let chunks_j4 = ruvid::downloader::http_range::plan_chunks(total_size as u64, 4);
    let mut ckpt_j4 = DownloadCheckpoint::new(
        &url,
        "direct-0",
        Some("\"v1\"".to_string()),
        Some("Mon, 16 Nov 2026 00:00:00 GMT".to_string()),
        Some(total_size as u64),
        chunks_j4[0].len(),
        chunks_j4.len(),
    );
    ckpt_j4.completed_chunks.insert(0);
    ckpt_j4.save_atomic(&ckpt2).unwrap();

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &url,
            "--jobs",
            "2",
            "--resume",
            "-o",
            out2.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!out2.exists());

    // 3. Partial file byte-count mismatch against checkpoint
    let out3 = tmp.path().join("part_size_mismatch.mp4");
    let part3 = tmp.path().join("part_size_mismatch.mp4.part");
    let ckpt3 = tmp.path().join("part_size_mismatch.mp4.part.ruvid.json");
    std::fs::write(&part3, vec![0u8; total_size - 10]).unwrap();
    ckpt_j4.save_atomic(&ckpt3).unwrap();

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &url,
            "--jobs",
            "4",
            "--resume",
            "-o",
            out3.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!out3.exists());

    // 4. Server byte-count mismatch between probe (206 Content-Range /1000) and GET (200 with 400 bytes)
    let mut probe_1000 = RoutePayload::bytes(vec![b'A'; 1000], "video/mp4");
    probe_1000.accept_ranges = false;
    let mut body_400 = RoutePayload::bytes(vec![b'B'; 400], "video/mp4");
    body_400.accept_ranges = false;
    body_400.honor_range = false;
    server.set_sequence("/mismatch.mp4", vec![probe_1000, body_400]);

    let out4 = tmp.path().join("mismatch.mp4");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/mismatch.mp4", server.base_url),
            "-o",
            out4.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!out4.exists());

    // 5. Corrupt segmented HLS checkpoint must fail with exit code 8
    server.set_route(
        "/seg_check.m3u8",
        RoutePayload::text(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXTINF:2.0,\n/s0.ts\n#EXT-X-ENDLIST\n",
            "application/vnd.apple.mpegurl",
        ),
    );
    server.set_route(
        "/s0.ts",
        RoutePayload::bytes(b"SEG0".to_vec(), "video/mp2t"),
    );
    let hls_out = tmp.path().join("seg_corrupt.ts");
    let hls_seg_dir = ruvid::storage::segment_temp_dir(&hls_out);
    std::fs::create_dir_all(&hls_seg_dir).unwrap();
    std::fs::write(
        hls_seg_dir.join("checkpoint.ruvid.json"),
        "{CORRUPT_SEG_CKPT",
    )
    .unwrap();
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/seg_check.m3u8", server.base_url),
            "--resume",
            "-o",
            hls_out.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!hls_out.exists());

    // 6. Direct unit verification of commit_part_file byte-count mismatch (RuviError::Integrity => exit code 8)
    let part5 = tmp.path().join("direct.part");
    std::fs::write(&part5, b"12345").unwrap();
    let err = commit_part_file(
        &part5,
        &tmp.path().join("direct.mp4"),
        Some(99),
        false,
        None,
    )
    .unwrap_err();
    assert_eq!(err.exit_code().code(), 8);
}

#[tokio::test]
async fn pure_rust_youtube_yt_initial_player_response_html_fixture_info_and_get() {
    let server = MockHttpServer::start().await;
    let video_720 = b"YOUTUBE_HTML_720P_MP4_PAYLOAD".to_vec();
    let video_360 = b"YOUTUBE_HTML_360P_MP4_PAYLOAD".to_vec();
    let vtt_en = "WEBVTT\n\n00:00:00.000 --> 00:00:03.000\nPure Rust YouTube caption\n";

    server.set_route(
        "/ytmedia/720.mp4",
        RoutePayload::bytes(video_720.clone(), "video/mp4"),
    );
    server.set_route(
        "/ytmedia/360.mp4",
        RoutePayload::bytes(video_360, "video/mp4"),
    );
    server.set_route("/ytmedia/en.vtt", RoutePayload::text(vtt_en, "text/vtt"));

    let html = format!(
        r#"<!doctype html>
<html>
<head><title>Ignored Page Title</title></head>
<body>
<script>
var ytInitialPlayerResponse = {{
  "playabilityStatus": {{ "status": "OK" }},
  "videoDetails": {{
    "videoId": "HTMLFIX1234",
    "title": "Pure Rust YouTube HTML Fixture"
  }},
  "streamingData": {{
    "formats": [
      {{
        "itag": 18,
        "url": "{base}/ytmedia/360.mp4?sig=SIG_360_SECRET&key=KEY_360_SECRET",
        "mimeType": "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"",
        "width": 640,
        "height": 360,
        "bitrate": 500000,
        "fps": 30,
        "contentLength": "{len360}"
      }},
      {{
        "itag": 22,
        "url": "{base}/ytmedia/720.mp4?sig=SIG_720_SECRET&key=KEY_720_SECRET",
        "mimeType": "video/mp4; codecs=\"avc1.64001F, mp4a.40.2\"",
        "width": 1280,
        "height": 720,
        "bitrate": 1500000,
        "fps": 30,
        "contentLength": "{len720}"
      }}
    ]
  }},
  "captions": {{
    "playerCaptionsTracklistRenderer": {{
      "captionTracks": [
        {{
          "baseUrl": "{base}/ytmedia/en.vtt?token=CAP_TOKEN_SECRET",
          "languageCode": "en",
          "name": {{ "simpleText": "English" }}
        }}
      ]
    }}
  }}
}};
</script>
</body>
</html>"#,
        base = server.base_url,
        len360 = b"YOUTUBE_HTML_360P_MP4_PAYLOAD".len(),
        len720 = video_720.len(),
    );

    server.set_route("/watch", RoutePayload::text(&html, "text/html"));
    let watch_url = format!("{}/watch?v=HTMLFIX1234", server.base_url);

    let tmp = tempfile::tempdir().unwrap();

    // 1. `ruvid info --json` resolves streams & captions in pure Rust and redacts stream/caption tokens
    let info_out = common::ruvid_cmd(tmp.path())
        .args(["info", &watch_url, "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pure Rust YouTube HTML Fixture"))
        .stdout(predicate::str::contains("itag-22"))
        .stdout(predicate::str::contains("itag-18"))
        .stdout(predicate::str::contains(r#""language": "en""#))
        .get_output()
        .stdout
        .clone();
    let info_text = String::from_utf8(info_out).unwrap();
    for secret in [
        "SIG_360_SECRET",
        "KEY_360_SECRET",
        "SIG_720_SECRET",
        "KEY_720_SECRET",
        "CAP_TOKEN_SECRET",
    ] {
        assert!(
            !info_text.contains(secret),
            "YouTube info --json leaked '{secret}': {info_text}"
        );
    }

    // 2. `ruvid get` downloads selected 720p stream + English subtitle track + writes manifest
    let tpl = tmp.path().join("%(title)s.%(ext)s");
    let manifest_path = tmp.path().join("yt_html.ruvi.json");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &watch_url,
            "-f",
            "720p",
            "--subtitles",
            "--subtitles-lang",
            "en",
            "-o",
            tpl.to_str().unwrap(),
            "--write-manifest",
            manifest_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-22""#));

    let expected_media = tmp.path().join("Pure Rust YouTube HTML Fixture.mp4");
    let expected_sub = tmp.path().join("Pure Rust YouTube HTML Fixture.en.vtt");
    assert_eq!(std::fs::read(&expected_media).unwrap(), video_720);
    assert_eq!(std::fs::read_to_string(&expected_sub).unwrap(), vtt_en);
    assert!(manifest_path.exists());
}

#[tokio::test]
async fn pure_rust_youtube_simulated_innertube_player_flow_on_mock_server() {
    let server = MockHttpServer::start().await;
    let video_bytes = b"INNERTUBE_ANDROID_720P_BYTES".to_vec();
    let sub_tr = "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nMerhaba Rust\n";

    server.set_route(
        "/innertube_media/720.mp4",
        RoutePayload::bytes(video_bytes.clone(), "video/mp4"),
    );
    server.set_route(
        "/innertube_media/tr.vtt",
        RoutePayload::text(sub_tr, "text/vtt"),
    );

    // Watch HTML carries VISITOR_DATA and a web ytInitialPlayerResponse with ciphered-only format,
    // triggering pure-Rust Innertube POST /youtubei/v1/player resolution on MockHttpServer.
    let watch_html = r#"<!doctype html>
<html>
<head><title>Watch</title></head>
<body>
<script>
var ytcfg = {"VISITOR_DATA":"VISITOR_MOCK_99"};
var ytInitialPlayerResponse = {
  "playabilityStatus": { "status": "OK" },
  "videoDetails": { "videoId": "INNTUBE99", "title": "Web Ciphered Title" },
  "streamingData": {
    "formats": [
      { "itag": 18, "signatureCipher": "s=ABC&sp=sig&url=https%3A%2F%2Fexample.invalid" }
    ]
  }
};
</script>
</body>
</html>"#;
    server.set_route("/watch", RoutePayload::text(watch_html, "text/html"));

    let innertube_json = serde_json::json!({
        "playabilityStatus": { "status": "OK" },
        "videoDetails": {
            "videoId": "INNTUBE99",
            "title": "Innertube Simulated Video"
        },
        "streamingData": {
            "formats": [
                {
                    "itag": 22,
                    "url": format!("{}/innertube_media/720.mp4?token=INN_TOK_SECRET", server.base_url),
                    "mimeType": "video/mp4; codecs=\"avc1.64001F, mp4a.40.2\"",
                    "width": 1280,
                    "height": 720,
                    "bitrate": 1800000,
                    "fps": 30,
                    "contentLength": video_bytes.len().to_string()
                }
            ],
            "adaptiveFormats": [
                {
                    "itag": 140,
                    "url": format!("{}/innertube_media/audio.m4a", server.base_url),
                    "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                    "bitrate": 128000
                }
            ]
        },
        "captions": {
            "playerCaptionsTracklistRenderer": {
                "captionTracks": [
                    {
                        "baseUrl": format!("{}/innertube_media/tr.vtt", server.base_url),
                        "languageCode": "tr",
                        "name": { "simpleText": "Turkish" }
                    }
                ]
            }
        }
    });
    server.set_route(
        "/youtubei/v1/player",
        RoutePayload::text(&innertube_json.to_string(), "application/json"),
    );

    let watch_url = format!("{}/watch?v=INNTUBE99", server.base_url);
    let tmp = tempfile::tempdir().unwrap();

    // 1. `ruvid info` hits `/youtubei/v1/player` and redacts Innertube tokens
    server.clear_requests();
    common::ruvid_cmd(tmp.path())
        .args(["info", &watch_url, "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Innertube Simulated Video"))
        .stdout(predicate::str::contains("itag-22"))
        .stdout(predicate::str::contains("itag-140"))
        .stdout(predicate::str::contains("INN_TOK_SECRET").not());

    let info_reqs = server.request_log();
    assert!(
        info_reqs
            .iter()
            .any(|r| r.starts_with("/youtubei/v1/player")),
        "expected Innertube /youtubei/v1/player request, got {info_reqs:?}"
    );

    // 2. `ruvid get` resolves via Innertube and downloads video + subtitles
    server.clear_requests();
    let out_mp4 = tmp.path().join("innertube.mp4");
    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &watch_url,
            "-f",
            "itag-22",
            "--subtitles",
            "--subtitles-lang",
            "tr",
            "-o",
            out_mp4.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "itag-22""#));

    assert_eq!(std::fs::read(&out_mp4).unwrap(), video_bytes);
    let sub_out = tmp.path().join("innertube.tr.vtt");
    assert_eq!(std::fs::read_to_string(sub_out).unwrap(), sub_tr);

    // 3. Android -> iOS HLS fallback when Android Innertube returns LOGIN_REQUIRED
    let android_denied = serde_json::json!({
        "playabilityStatus": { "status": "LOGIN_REQUIRED", "reason": "Sign in" }
    });
    let ios_hls = serde_json::json!({
        "playabilityStatus": { "status": "OK" },
        "videoDetails": { "videoId": "INNTUBE99", "title": "Innertube iOS HLS Fallback" },
        "streamingData": {
            "hlsManifestUrl": format!("{}/yt_hls/media.m3u8", server.base_url)
        }
    });
    server.set_sequence(
        "/youtubei/v1/player",
        vec![
            RoutePayload::text(&android_denied.to_string(), "application/json"),
            RoutePayload::text(&ios_hls.to_string(), "application/json"),
        ],
    );
    server.set_route(
        "/yt_hls/media.m3u8",
        RoutePayload::text(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXTINF:2.0,\n/yt_hls/s0.ts\n#EXTINF:2.0,\n/yt_hls/s1.ts\n#EXT-X-ENDLIST\n",
            "application/vnd.apple.mpegurl",
        ),
    );
    server.set_route(
        "/yt_hls/s0.ts",
        RoutePayload::bytes(b"IOS-HLS-0-".to_vec(), "video/mp2t"),
    );
    server.set_route(
        "/yt_hls/s1.ts",
        RoutePayload::bytes(b"IOS-HLS-1".to_vec(), "video/mp2t"),
    );

    let ios_out = tmp.path().join("ios_hls.ts");
    common::ruvid_cmd(tmp.path())
        .args(["get", &watch_url, "-o", ios_out.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""stream_id": "yt-hls""#));
    assert_eq!(std::fs::read(&ios_out).unwrap(), b"IOS-HLS-0-IOS-HLS-1");
}
