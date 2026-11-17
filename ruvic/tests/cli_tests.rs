mod common;

use predicates::prelude::*;

#[test]
fn help_and_version_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    common::ruvic_cmd(tmp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("probe"))
        .stdout(predicate::str::contains("convert"))
        .stdout(predicate::str::contains("remux"));

    common::ruvic_cmd(tmp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ruvic"));
}

#[test]
fn config_init_show_validate_and_preset_commands_work() {
    let tmp = tempfile::tempdir().unwrap();

    common::ruvic_cmd(tmp.path())
        .args(["config", "init"])
        .assert()
        .success();

    common::ruvic_cmd(tmp.path())
        .args(["config", "validate", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#));

    common::ruvic_cmd(tmp.path())
        .args(["preset", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("web-mp4"))
        .stdout(predicate::str::contains("archive-mkv"));

    common::ruvic_cmd(tmp.path())
        .args(["preset", "show", "web-mp4", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("libx264"));

    let bad_preset = tmp.path().join("bad-preset.toml");
    std::fs::write(
        &bad_preset,
        r#"
name = "broken"
description = "incompatible"
[container]
format = "mp4"
[video]
codec = "vp9"
"#,
    )
    .unwrap();

    common::ruvic_cmd(tmp.path())
        .args(["preset", "validate", bad_preset.to_str().unwrap()])
        .assert()
        .code(4);
}

#[test]
fn doctor_reports_native_engine_and_optional_ffmpeg_override() {
    let tmp = tempfile::tempdir().unwrap();

    // Even with an empty PATH and no external ffmpeg, built-in native Rust engine is ready
    common::ruvic_cmd(tmp.path())
        .env("PATH", tmp.path())
        .env_remove("RUVIC_FFMPEG_PATH")
        .env_remove("RUVIC_FFPROBE_PATH")
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains(r#""engine": "native-rust""#))
        .stdout(predicate::str::contains(r#""ffmpeg_found": false"#));

    let pair = common::make_fake_ffmpeg_pair();
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains(r#""engine": "ffmpeg-subprocess""#))
        .stdout(predicate::str::contains("7.1-ruvic-fake"));
}

#[test]
fn completions_subcommand_generates_shell_scripts() {
    let tmp = tempfile::tempdir().unwrap();

    common::ruvic_cmd(tmp.path())
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"))
        .stdout(predicate::str::contains("ruvic"));

    common::ruvic_cmd(tmp.path())
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ruvic"));
}
