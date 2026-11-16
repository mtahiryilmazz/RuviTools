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
