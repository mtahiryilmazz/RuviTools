mod common;

use predicates::prelude::*;

#[test]
fn help_version_config_and_doctor_succeed() {
    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("playlist"));

    common::ruvid_cmd(tmp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ruvid"));

    common::ruvid_cmd(tmp.path())
        .args(["config", "init"])
        .assert()
        .success();

    common::ruvid_cmd(tmp.path())
        .args(["config", "validate", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#));

    let dl_dir = tmp.path().join("downloads");
    let cfg_path = tmp.path().join("custom.toml");
    std::fs::write(
        &cfg_path,
        format!(
            "[download]\noutput_dir = \"{}\"\n",
            dl_dir.to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .unwrap();

    common::ruvid_cmd(tmp.path())
        .args(["--config", cfg_path.to_str().unwrap(), "doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""output_dir_writable": true"#));
}

#[test]
fn rejects_disallowed_schemes_and_private_host_by_default() {
    let tmp = tempfile::tempdir().unwrap();

    let mut cmd_ftp = assert_cmd::cargo::cargo_bin_cmd!("ruvid");
    cmd_ftp
        .env("RUVI_CONFIG_DIR", tmp.path().join("cfg"))
        .args(["info", "ftp://example.com/video.mp4"])
        .assert()
        .code(4);

    let mut cmd_ssrf = assert_cmd::cargo::cargo_bin_cmd!("ruvid");
    cmd_ssrf
        .env("RUVI_CONFIG_DIR", tmp.path().join("cfg"))
        .env_remove("RUVID_ALLOW_PRIVATE_NET")
        .args(["info", "http://127.0.0.1:9999/video.mp4"])
        .assert()
        .code(4);
}

#[test]
fn completions_subcommand_generates_shell_scripts() {
    let tmp = tempfile::tempdir().unwrap();

    common::ruvid_cmd(tmp.path())
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"))
        .stdout(predicate::str::contains("ruvid"));

    common::ruvid_cmd(tmp.path())
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ruvid"));
}

#[tokio::test]
async fn get_simulate_resolves_stream_and_filename_without_writing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let server = common::MockHttpServer::start().await;
    let payload_bytes = b"0123456789abcdef_mock_mp4_payload".to_vec();
    server.set_route(
        "/demo/sample_video.mp4",
        common::RoutePayload::bytes(payload_bytes.clone(), "video/mp4"),
    );

    let nonexistent_dir = tmp.path().join("should_not_be_created");
    let template = nonexistent_dir.join("%(title)s.%(ext)s");
    let url = format!("{}/demo/sample_video.mp4", server.base_url);

    let out = common::ruvid_cmd(tmp.path())
        .args([
            "get",
            "--simulate",
            "--json",
            "-o",
            template.to_str().unwrap(),
            &url,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["simulated"], true);
    assert_eq!(parsed["title"], "sample_video");
    assert_eq!(parsed["stream_id"], "direct-0");
    assert_eq!(parsed["container"], "mp4");
    assert_eq!(parsed["expected_bytes"], payload_bytes.len() as u64);
    assert!(
        parsed["output_path"]
            .as_str()
            .unwrap()
            .ends_with("sample_video.mp4")
    );
    assert!(
        !nonexistent_dir.exists(),
        "--simulate must not create directories or write files"
    );

    common::ruvid_cmd(tmp.path())
        .args(["get", "--simulate", "-o", template.to_str().unwrap(), &url])
        .assert()
        .success()
        .stdout(predicate::str::contains("[simulate]"))
        .stdout(predicate::str::contains("sample_video.mp4"));
    assert!(!nonexistent_dir.exists());
}

#[tokio::test]
async fn playlist_batch_file_and_passthrough_flags_work() {
    let tmp = tempfile::tempdir().unwrap();
    let server = common::MockHttpServer::start().await;
    server.set_route(
        "/first.mp4",
        common::RoutePayload::bytes(b"first_video_bytes".to_vec(), "video/mp4"),
    );
    server.set_route(
        "/second.mp4",
        common::RoutePayload::bytes(b"second_video_bytes".to_vec(), "video/mp4"),
    );

    let batch_path = tmp.path().join("urls.txt");
    let batch_contents = format!(
        "# Comment line should be ignored\n\n  {}/first.mp4  \n# another comment\n{}/second.mp4\n",
        server.base_url, server.base_url
    );
    std::fs::write(&batch_path, &batch_contents).unwrap();

    let out_dir = tmp.path().join("batch_out");
    let out = common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            "--batch-file",
            batch_path.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
            "--write-manifest",
            "--subtitles",
            "--subtitles-lang",
            "en",
            "--sub-format",
            "srt",
            "--auto-subs",
            "--audio-lang",
            "en",
            "--split-chapters",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["total_selected"], 2);
    assert_eq!(report["succeeded"], 2);
    assert_eq!(report["failed"], 0);

    let manifest_count = std::fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".manifest.json"))
        .count();
    assert_eq!(manifest_count, 2);

    // Also test stdin ('-') batch-file input
    let stdin_dir = tmp.path().join("stdin_out");
    let stdin_out = common::ruvid_cmd(tmp.path())
        .args([
            "playlist",
            "-a",
            "-",
            "-o",
            stdin_dir.to_str().unwrap(),
            "--json",
        ])
        .write_stdin(format!("# stdin list\n{}/first.mp4\n", server.base_url))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdin_report: serde_json::Value = serde_json::from_slice(&stdin_out).unwrap();
    assert_eq!(stdin_report["ok"], true);
    assert_eq!(stdin_report["total_selected"], 1);
    assert_eq!(stdin_report["succeeded"], 1);

    // Missing both URL and --batch-file should fail CLI argument parsing (ExitCode::ConfigOrArgs = 2)
    common::ruvid_cmd(tmp.path())
        .args(["playlist"])
        .assert()
        .code(2);
}
