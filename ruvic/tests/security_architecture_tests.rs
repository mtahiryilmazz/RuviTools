mod common;

use std::path::Path;

use predicates::prelude::*;
use ruvic::backend::ffmpeg::process::sanitize_log_line;

fn assert_no_temp_files(dir: &Path) {
    let leftovers: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".ruvic-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "expected zero .ruvic-tmp-* files in {}, found: {:?}",
        dir.display(),
        leftovers
    );
}

#[test]
fn shell_metacharacter_filenames_execute_without_shell_injection() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();

    let mut special_names = vec![
        "clip & echo pwned.mp4",
        "video;id.mp4",
        "quoted 'single' & spaces.mp4",
    ];
    if cfg!(not(windows)) {
        special_names.push("video \"double\" & ;id.mp4");
    }

    for (idx, name) in special_names.iter().enumerate() {
        let input = tmp.path().join(name);
        std::fs::write(&input, b"source bytes").unwrap();
        let out_name = format!("out_{idx}_{name}");
        let output = tmp.path().join(&out_name);
        let args_log = tmp.path().join(format!("recorded_args_{idx}.txt"));

        common::ruvic_cmd(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .env("RUVIC_FAKE_RECORD_ARGS", &args_log)
            .args([
                "convert",
                input.to_str().unwrap(),
                "--preset",
                "web-mp4",
                "-o",
                output.to_str().unwrap(),
                "--json",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(r#""ok": true"#));

        assert!(output.exists(), "expected output to exist for {name}");
        assert_no_temp_files(tmp.path());

        let recorded = std::fs::read_to_string(&args_log).unwrap();
        let argv: Vec<&str> = recorded.lines().collect();

        // Must never invoke a shell wrapper
        assert!(
            !argv
                .iter()
                .any(|a| matches!(*a, "cmd" | "cmd.exe" | "/C" | "/c" | "sh" | "-c")),
            "unexpected shell wrapper in recorded argv: {argv:?}"
        );

        // Input filename with metacharacters must arrive verbatim as one single argv token after -i
        let input_pos = argv
            .iter()
            .position(|a| *a == "-i")
            .expect("expected -i flag in argv");
        assert_eq!(
            argv.get(input_pos + 1).copied(),
            Some(input.to_string_lossy().as_ref()),
            "input path was split or altered in argv: {argv:?}"
        );

        // Final output arg must be the single temp output path in the target directory
        let last_arg = argv.last().copied().unwrap_or_default();
        assert!(
            last_arg.contains(".ruvic-tmp-"),
            "expected temp output path as final arg, got: {last_arg}"
        );
    }
}

#[test]
fn same_input_and_output_path_rejected_across_all_operations() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();

    let file_name = "file.mp4";
    let abs_file = tmp.path().join(file_name);
    let sentinel = b"DO_NOT_OVERWRITE_SOURCE";
    std::fs::write(&abs_file, sentinel).unwrap();

    // Exact same path, relative ./file.mp4 vs absolute, non-existent parent/../file.mp4,
    // and Windows uppercase variant.
    let mut same_targets: Vec<String> = vec![
        abs_file.to_string_lossy().into_owned(),
        format!("./{file_name}"),
        format!("nonexistent_sub/../{file_name}"),
    ];
    if cfg!(windows) {
        same_targets.push(tmp.path().join("FILE.MP4").to_string_lossy().into_owned());
    }

    for out_arg in &same_targets {
        // convert
        common::ruvic_cmd(tmp.path())
            .current_dir(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args([
                "convert",
                "./file.mp4",
                "--preset",
                "web-mp4",
                "-o",
                out_arg,
                "--overwrite",
            ])
            .assert()
            .code(2);

        // remux
        common::ruvic_cmd(tmp.path())
            .current_dir(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args(["remux", "./file.mp4", "-o", out_arg, "--overwrite"])
            .assert()
            .code(2);

        // audio
        common::ruvic_cmd(tmp.path())
            .current_dir(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args([
                "audio",
                "./file.mp4",
                "--copy",
                "-o",
                out_arg,
                "--overwrite",
            ])
            .assert()
            .code(2);

        // trim
        common::ruvic_cmd(tmp.path())
            .current_dir(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args([
                "trim",
                "./file.mp4",
                "--start",
                "0",
                "--duration",
                "3",
                "-o",
                out_arg,
                "--overwrite",
            ])
            .assert()
            .code(2);

        // thumbnail
        common::ruvic_cmd(tmp.path())
            .current_dir(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args([
                "thumbnail",
                "./file.mp4",
                "--at",
                "00:00:01",
                "-o",
                out_arg,
                "--overwrite",
            ])
            .assert()
            .code(2);
    }

    assert!(!tmp.path().join("nonexistent_sub").exists());
    assert_eq!(std::fs::read(&abs_file).unwrap(), sentinel);
    assert_no_temp_files(tmp.path());
}

#[test]
fn overwrite_protection_enforced_across_remux_audio_trim_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();

    let input = tmp.path().join("input.mp4");
    std::fs::write(&input, b"source media").unwrap();

    let remux_out = tmp.path().join("existing.mkv");
    let audio_out = tmp.path().join("existing.mp3");
    let trim_out = tmp.path().join("existing_clip.mp4");
    let thumb_out = tmp.path().join("existing_thumb.jpg");

    let original_bytes = b"EXISTING_PROTECTED_CONTENT";
    for dest in [&remux_out, &audio_out, &trim_out, &thumb_out] {
        std::fs::write(dest, original_bytes).unwrap();
    }

    let cases: Vec<(&Path, Vec<&str>)> = vec![
        (
            remux_out.as_path(),
            vec![
                "remux",
                input.to_str().unwrap(),
                "-o",
                remux_out.to_str().unwrap(),
            ],
        ),
        (
            audio_out.as_path(),
            vec![
                "audio",
                input.to_str().unwrap(),
                "--to",
                "mp3",
                "-o",
                audio_out.to_str().unwrap(),
            ],
        ),
        (
            trim_out.as_path(),
            vec![
                "trim",
                input.to_str().unwrap(),
                "--start",
                "00:00:01",
                "--duration",
                "4",
                "-o",
                trim_out.to_str().unwrap(),
            ],
        ),
        (
            thumb_out.as_path(),
            vec![
                "thumbnail",
                input.to_str().unwrap(),
                "--at",
                "00:00:02",
                "-o",
                thumb_out.to_str().unwrap(),
            ],
        ),
    ];

    for (dest, base_args) in cases {
        // Without --overwrite -> exit code 5 and original content preserved
        common::ruvic_cmd(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args(&base_args)
            .assert()
            .code(5);
        assert_eq!(std::fs::read(dest).unwrap(), original_bytes);
        assert_no_temp_files(tmp.path());

        // With --overwrite -> succeeds and replaces destination
        let mut overwrite_args = base_args.clone();
        overwrite_args.push("--overwrite");
        common::ruvic_cmd(tmp.path())
            .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
            .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
            .args(&overwrite_args)
            .assert()
            .success();
        assert_ne!(std::fs::read(dest).unwrap(), original_bytes);
        assert_no_temp_files(tmp.path());
    }
}

#[test]
fn empty_and_corrupt_output_fail_with_integrity_code_8_and_clean_temp_files() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();

    let input = tmp.path().join("input.mp4");
    std::fs::write(&input, b"source media").unwrap();

    // 1. empty_output on convert -> exit code 8, no output, no leftover .ruvic-tmp-*
    let empty_out = tmp.path().join("empty.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_MODE", "empty_output")
        .args([
            "convert",
            input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            empty_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .code(8)
        .stdout(predicate::str::contains(r#""category": "integrity""#));
    assert!(!empty_out.exists());
    assert_no_temp_files(tmp.path());

    // 2. empty_output on thumbnail -> exit code 8, no output, no leftover .ruvic-tmp-*
    let empty_thumb = tmp.path().join("empty_thumb.jpg");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_MODE", "empty_output")
        .args([
            "thumbnail",
            input.to_str().unwrap(),
            "--at",
            "00:00:01",
            "-o",
            empty_thumb.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!empty_thumb.exists());
    assert_no_temp_files(tmp.path());

    // 3. corrupt_output with --overwrite must fail with exit code 8, preserve existing target, and clean temp
    let protected_out = tmp.path().join("protected.mp4");
    let keep_bytes = b"VALID_EXISTING_DESTINATION";
    std::fs::write(&protected_out, keep_bytes).unwrap();

    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_MODE", "corrupt_output")
        .args([
            "convert",
            input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            protected_out.to_str().unwrap(),
            "--overwrite",
            "--json",
        ])
        .assert()
        .code(8)
        .stdout(predicate::str::contains(r#""category": "integrity""#));
    assert_eq!(std::fs::read(&protected_out).unwrap(), keep_bytes);
    assert_no_temp_files(tmp.path());
}

#[test]
fn manifest_v1_accepts_unknown_fields_and_rejects_v2() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();

    let media = tmp.path().join("handoff_source.mp4");
    std::fs::write(&media, b"source media").unwrap();

    // Reject schema_version = "2.0" with exit code 4
    let v2_manifest = tmp.path().join("v2.ruvi.json");
    std::fs::write(
        &v2_manifest,
        serde_json::to_string(&serde_json::json!({
            "schema_version": "2.0",
            "output": { "path": media.to_string_lossy() }
        }))
        .unwrap(),
    )
    .unwrap();

    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "probe",
            "--manifest",
            v2_manifest.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .code(4)
        .stdout(predicate::str::contains(r#""category": "unsupported""#));

    // Accept schema_version = "1" with unknown extra fields (top-level and nested)
    let v1_manifest = tmp.path().join("v1.ruvi.json");
    std::fs::write(
        &v1_manifest,
        serde_json::to_string(&serde_json::json!({
            "schema_version": "1",
            "producer": { "name": "ruvid", "version": "0.1.0", "extra_nested": 42 },
            "source": { "url": "https://example.invalid/video.mp4", "extra_source_tag": "ok" },
            "output": {
                "path": media.to_string_lossy(),
                "container": "mp4",
                "size_bytes": 12,
                "future_output_checksum": "sha256:abc"
            },
            "streams": [
                { "kind": "video", "codec": "h264", "unknown_stream_flag": true }
            ],
            "subtitles": [],
            "unknown_top_level": { "nested": [1, 2, 3] }
        }))
        .unwrap(),
    )
    .unwrap();

    let out_mp4 = tmp.path().join("from_manifest.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "convert",
            "--manifest",
            v1_manifest.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            out_mp4.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#));
    assert!(out_mp4.exists());
    assert_no_temp_files(tmp.path());
}

#[test]
fn log_redaction_and_explicit_locator_override_guarantees() {
    // 1. Multiple secrets, userinfo, and Bearer headers are redacted while non-secret stats stay intact
    let line = "GET https://alice:s3cr3t@cdn.example.invalid/v.mp4?token=tok1&X-Amz-Signature=sig2&api_key=k3&keyint=250 Authorization: Bearer jwt.tok.here";
    let redacted = sanitize_log_line(line);
    assert!(!redacted.contains("alice:s3cr3t"));
    assert!(!redacted.contains("tok1"));
    assert!(!redacted.contains("sig2"));
    assert!(!redacted.contains("k3"));
    assert!(!redacted.contains("jwt.tok.here"));
    assert!(redacted.contains("keyint=250"));
    assert!(redacted.contains("[REDACTED]"));

    // 2. Non-existent explicit --ffmpeg override must fail instead of falling back to PATH
    let tmp = tempfile::tempdir().unwrap();
    let missing_bin = tmp.path().join("missing_ffmpeg.exe");
    common::ruvic_cmd(tmp.path())
        .args([
            "--ffmpeg",
            missing_bin.to_str().unwrap(),
            "doctor",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ffmpeg_found": false"#))
        .stdout(predicate::str::contains(r#""ok": false"#));
}
