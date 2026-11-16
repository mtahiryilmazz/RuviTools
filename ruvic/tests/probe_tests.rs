mod common;

use predicates::prelude::*;

#[test]
fn probe_parses_media_info_and_supports_manifest_input() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();
    let media = tmp.path().join("sample.mp4");
    std::fs::write(&media, b"dummy mp4").unwrap();

    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args(["probe", media.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1920"))
        .stdout(predicate::str::contains("h264"))
        .stdout(predicate::str::contains("aac"));

    let manifest_path = tmp.path().join("handoff.ruvi.json");
    let manifest_json = serde_json::json!({
        "schema_version": "1",
        "producer": { "name": "ruvid", "version": "0.1.0" },
        "source": { "url": "https://example.invalid/video.mp4", "title": "Sample" },
        "output": { "path": media.to_string_lossy(), "container": "mp4", "size_bytes": 9 },
        "streams": [],
        "created_at": "2026-11-16T00:00:00Z",
        "future_unknown_field": true
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&manifest_json).unwrap(),
    )
    .unwrap();

    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "probe",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("h264"));
}

#[test]
fn probe_missing_input_returns_filesystem_error() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();
    let missing = tmp.path().join("does_not_exist.mp4");

    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args(["probe", missing.to_str().unwrap(), "--json"])
        .assert()
        .code(5)
        .stdout(predicate::str::contains(r#""ok": false"#));
}
