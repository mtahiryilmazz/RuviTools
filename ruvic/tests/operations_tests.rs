mod common;

use predicates::prelude::*;

#[test]
fn convert_remux_audio_trim_thumbnail_and_batch_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();
    let input = tmp.path().join("input.mp4");
    std::fs::write(&input, b"source bytes").unwrap();

    let args_log = tmp.path().join("recorded_args.txt");
    let out_mp4 = tmp.path().join("web.mp4");

    // 1. Convert with web-mp4 preset
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
            out_mp4.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("libx264"))
        .stdout(predicate::str::contains(r#""ok": true"#));
    assert!(out_mp4.exists());

    let recorded = std::fs::read_to_string(&args_log).unwrap();
    assert!(recorded.contains("-movflags"));
    assert!(recorded.contains("+faststart"));

    // 2. Overwrite protection rejects when output exists without --overwrite
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "convert",
            input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            out_mp4.to_str().unwrap(),
        ])
        .assert()
        .code(5);

    // 3. Same input/output path rejected with code 2
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "remux",
            input.to_str().unwrap(),
            "-o",
            input.to_str().unwrap(),
        ])
        .assert()
        .code(2);

    // 4. Valid remux mp4 -> mkv
    let out_mkv = tmp.path().join("remuxed.mkv");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "remux",
            input.to_str().unwrap(),
            "-o",
            out_mkv.to_str().unwrap(),
            "--copy",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""operation": "remux""#));
    assert!(out_mkv.exists());

    // 5. Audio extraction
    let out_mp3 = tmp.path().join("track.mp3");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "audio",
            input.to_str().unwrap(),
            "--to",
            "mp3",
            "-o",
            out_mp3.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("libmp3lame"));
    assert!(out_mp3.exists());

    // 6. Trim
    let out_clip = tmp.path().join("clip.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "trim",
            input.to_str().unwrap(),
            "--start",
            "00:00:02",
            "--duration",
            "5",
            "-o",
            out_clip.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""operation": "trim""#));
    assert!(out_clip.exists());

    // 7. Thumbnail
    let out_thumb = tmp.path().join("thumb.jpg");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "thumbnail",
            input.to_str().unwrap(),
            "--at",
            "00:00:03",
            "-o",
            out_thumb.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""operation": "thumbnail""#));
    assert!(out_thumb.exists());

    // 8. Batch conversion
    let batch_in = tmp.path().join("batch_in");
    let batch_out = tmp.path().join("batch_out");
    std::fs::create_dir_all(&batch_in).unwrap();
    std::fs::write(batch_in.join("a.mkv"), b"a").unwrap();
    std::fs::write(batch_in.join("b.mkv"), b"b").unwrap();

    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "batch",
            batch_in.to_str().unwrap(),
            "-o",
            batch_out.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""succeeded": 2"#));
    assert!(batch_out.join("a.mp4").exists());
    assert!(batch_out.join("b.mp4").exists());
}

#[test]
fn incompatible_remux_corrupt_output_encoder_error_and_hwaccel_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let pair = common::make_fake_ffmpeg_pair();

    // 1. Incompatible container/codec during remux (vp9/opus -> mp4) -> exit code 4
    let vp9_input = tmp.path().join("vp9_clip.webm");
    std::fs::write(&vp9_input, b"vp9").unwrap();
    let bad_mp4 = tmp.path().join("bad.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "remux",
            vp9_input.to_str().unwrap(),
            "-o",
            bad_mp4.to_str().unwrap(),
        ])
        .assert()
        .code(4);
    assert!(!bad_mp4.exists());

    let valid_input = tmp.path().join("ok.mp4");
    std::fs::write(&valid_input, b"ok").unwrap();

    // 2. Corrupt output triggers validation failure (exit code 8) and leaves no final file
    let corrupt_out = tmp.path().join("corrupt.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_MODE", "corrupt_output")
        .args([
            "convert",
            valid_input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            corrupt_out.to_str().unwrap(),
        ])
        .assert()
        .code(8);
    assert!(!corrupt_out.exists());

    // 3. Encoder error returns exit code 6 and redacts token in error message
    let failed_out = tmp.path().join("failed.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_MODE", "fail_encoder")
        .args([
            "convert",
            valid_input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            failed_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .code(6)
        .stdout(predicate::str::contains("[REDACTED]"))
        .stdout(predicate::str::contains("supersecret123").not());

    // 4. Requested hwaccel unavailable falls back to CPU and reports reason
    let fallback_out = tmp.path().join("fallback.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .args([
            "convert",
            valid_input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "--hwaccel",
            "nvenc",
            "-o",
            fallback_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("falling back to CPU encoder"))
        .stdout(predicate::str::contains("libx264"));

    // 5. Available hwaccel picks h264_nvenc
    let nvenc_out = tmp.path().join("nvenc.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_HAS_NVENC", "1")
        .args([
            "convert",
            valid_input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "--hwaccel",
            "nvenc",
            "-o",
            nvenc_out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("h264_nvenc"));

    // 6. Timeout kills child process and leaves no temp/final file
    let timeout_cfg = tmp.path().join("timeout.toml");
    std::fs::write(&timeout_cfg, "[backend]\ntimeout_seconds = 1\n").unwrap();
    let timed_out_file = tmp.path().join("timed_out.mp4");
    common::ruvic_cmd(tmp.path())
        .env("RUVIC_FFMPEG_PATH", &pair.ffmpeg)
        .env("RUVIC_FFPROBE_PATH", &pair.ffprobe)
        .env("RUVIC_FAKE_SLEEP_MS", "3500")
        .args([
            "--config",
            timeout_cfg.to_str().unwrap(),
            "convert",
            valid_input.to_str().unwrap(),
            "--preset",
            "web-mp4",
            "-o",
            timed_out_file.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .code(6)
        .stdout(predicate::str::contains("exceeded timeout"));
    assert!(!timed_out_file.exists());
}
