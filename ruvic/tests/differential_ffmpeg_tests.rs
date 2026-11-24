//! Differential ("oracle") verification of the native engine against real `ffmpeg`/`ffprobe`.
//!
//! These tests are **opt-in by design**: `ruvic` ships zero external binaries and the
//! default `cargo test` pass must stay hermetic. They only run when
//!
//! ```text
//! RUVIC_DIFF=1 cargo test --test differential_ffmpeg_tests -- --nocapture
//! ```
//!
//! and silently skip when no real `ffmpeg`/`ffprobe` is installed. `RUVIC_DIFF_FFMPEG`
//! and `RUVIC_DIFF_FFPROBE` override the binary lookup.
//!
//! What is compared, and against what:
//!
//! | Test | ruvic side | oracle side |
//! |------|-----------|-------------|
//! | `diff_probe_matches_ffprobe` | `ruvic probe --json` | `ffprobe -show_format -show_streams` |
//! | `diff_remux_preserves_sample_tables` | `ruvic remux` | `ffprobe` frame counts + `ffmpeg -f null` decode |
//! | `diff_trim_matches_stream_copy` | `ruvic trim` | `ffmpeg -ss/-t -c copy` |
//! | `diff_dual_mux_rebuilds_dash_streams` | `ruvic remux --video --audio` | `ffprobe` stream set + decode |
//! | `diff_ntsc_timescale_has_no_drift` | `ruvic remux` @ 30000/1001 | `ffprobe` duration/frame count |
//!
//! Every ruvic output additionally passes the decode gate: `ffmpeg -v error -i out -f null -`
//! must exit 0 with completely empty stderr. That is the strongest cheap signal that a
//! rebuilt `stts`/`ctts`/`stsc`/`stco` table is still internally consistent.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

// ---------------------------------------------------------------------------
// Oracle plumbing
// ---------------------------------------------------------------------------

struct Oracle {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

impl Oracle {
    /// Resolve a real ffmpeg/ffprobe pair, honouring the override env vars first.
    fn detect() -> Option<Self> {
        let ffmpeg = find_exe("RUVIC_DIFF_FFMPEG", &["ffmpeg"])?;
        let ffprobe = find_exe("RUVIC_DIFF_FFPROBE", &["ffprobe"])?;
        Some(Self { ffmpeg, ffprobe })
    }

    fn ffmpeg(&self, args: &[&str]) {
        let output = Command::new(&self.ffmpeg)
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("failed to run {}: {err}", self.ffmpeg.display()));
        assert!(
            output.status.success(),
            "ffmpeg {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn probe(&self, file: &Path) -> Value {
        let output = Command::new(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(file)
            .output()
            .unwrap_or_else(|err| panic!("failed to run {}: {err}", self.ffprobe.display()));
        assert!(
            output.status.success(),
            "ffprobe failed on {}:\n{}",
            file.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("ffprobe JSON")
    }

    /// Real H.264 + AAC source clip at a given frame rate.
    fn make_source(&self, dir: &Path, name: &str, rate: &str, seconds: &str) -> PathBuf {
        let path = dir.join(name);
        let video = format!("testsrc2=size=320x240:rate={rate}");
        let audio = "sine=frequency=440:sample_rate=48000";
        self.ffmpeg(&[
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &video,
            "-f",
            "lavfi",
            "-i",
            audio,
            "-t",
            seconds,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "30",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            path.to_str().unwrap(),
        ]);
        path
    }

    /// Split a source clip into two fragmented single-track MP4s, the shape a DASH
    /// downloader (ruvid) hands over to `ruvic remux --video/--audio`.
    fn split_dash(&self, dir: &Path, src: &Path) -> (PathBuf, PathBuf) {
        let video = dir.join("dash_video.mp4");
        let audio = dir.join("dash_audio.mp4");
        let frag = "frag_keyframe+empty_moov+default_base_moof";
        for (map, out) in [("0:v:0", &video), ("0:a:0", &audio)] {
            self.ffmpeg(&[
                "-y",
                "-v",
                "error",
                "-i",
                src.to_str().unwrap(),
                "-map",
                map,
                "-c",
                "copy",
                "-movflags",
                frag,
                "-f",
                "mp4",
                out.to_str().unwrap(),
            ]);
        }
        (video, audio)
    }

    /// `ffmpeg -v error -i file -f null -`: any demux/decode warning fails the gate.
    fn decode_gate(&self, file: &Path) {
        let output = Command::new(&self.ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-f", "null", "-"])
            .output()
            .expect("run ffmpeg decode gate");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "decode gate failed for {}:\n{stderr}",
            file.display()
        );
        assert!(
            stderr.trim().is_empty(),
            "decoder reported errors for {}:\n{stderr}",
            file.display()
        );
    }
}

fn find_exe(env_key: &str, stems: &[&str]) -> Option<PathBuf> {
    if let Ok(raw) = std::env::var(env_key) {
        let candidate = PathBuf::from(raw);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let suffixes: &[&str] = if cfg!(windows) { &["", ".exe"] } else { &[""] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for stem in stems {
            for suffix in suffixes {
                let candidate = dir.join(format!("{stem}{suffix}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// `Some(oracle)` only when the suite is explicitly enabled *and* ffmpeg exists.
fn oracle() -> Option<Oracle> {
    if std::env::var("RUVIC_DIFF").ok().as_deref() != Some("1") {
        eprintln!("skip: set RUVIC_DIFF=1 to run differential tests against ffmpeg");
        return None;
    }
    match Oracle::detect() {
        Some(oracle) => Some(oracle),
        None => {
            eprintln!("skip: no ffmpeg/ffprobe found on PATH (set RUVIC_DIFF_FFMPEG/_FFPROBE)");
            None
        }
    }
}

macro_rules! require_oracle {
    () => {
        match oracle() {
            Some(o) => o,
            None => return,
        }
    };
}

/// A `ruvic` command with an empty `PATH`, so the run is guaranteed to use the
/// native engine instead of silently falling back to a system ffmpeg.
fn native_cmd(work: &Path) -> assert_cmd::Command {
    let empty = work.join("empty_path");
    std::fs::create_dir_all(&empty).unwrap();
    let mut cmd = common::ruvic_cmd(work);
    cmd.env("PATH", &empty)
        .env_remove("RUVIC_FFMPEG_PATH")
        .env_remove("RUVIC_FFPROBE_PATH");
    cmd
}

// ---------------------------------------------------------------------------
// JSON accessors
// ---------------------------------------------------------------------------

fn f64_at(value: &Value, path: &[&str]) -> Option<f64> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    cursor
        .as_f64()
        .or_else(|| cursor.as_str().and_then(|s| s.parse::<f64>().ok()))
}

fn u64_at(value: &Value, path: &[&str]) -> Option<u64> {
    f64_at(value, path).map(|v| v as u64)
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    cursor.as_str()
}

fn streams_of(value: &Value) -> &[Value] {
    value
        .get("streams")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn first_stream<'a>(value: &'a Value, codec_type: &str) -> Option<&'a Value> {
    streams_of(value)
        .iter()
        .find(|s| str_at(s, &["codec_type"]) == Some(codec_type))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// `ruvic probe` must describe the same container as `ffprobe`.
#[test]
fn diff_probe_matches_ffprobe() {
    let oracle = require_oracle!();
    let work = tempfile::tempdir().unwrap();
    let src = oracle.make_source(work.path(), "src_30fps.mp4", "30", "4");

    let output = native_cmd(work.path())
        .args(["probe", src.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let info: Value = serde_json::from_slice(&output).expect("ruvic probe JSON");
    let reference = oracle.probe(&src);

    let ruvic_dur = f64_at(&info, &["container", "duration_seconds"]).expect("ruvic duration");
    let ffprobe_dur = f64_at(&reference, &["format", "duration"]).expect("ffprobe duration");
    assert!(
        (ruvic_dur - ffprobe_dur).abs() <= 0.05,
        "duration drift: ruvic {ruvic_dur} vs ffprobe {ffprobe_dur}"
    );

    assert_eq!(
        streams_of(&info).len(),
        streams_of(&reference).len(),
        "stream count differs"
    );

    let ruvic_video = streams_of(&info)
        .iter()
        .find(|s| str_at(s, &["kind"]) == Some("video"))
        .expect("ruvic video stream");
    let ff_video = first_stream(&reference, "video").expect("ffprobe video stream");
    assert_eq!(
        str_at(ruvic_video, &["codec_name"]),
        str_at(ff_video, &["codec_name"]),
        "video codec differs"
    );
    assert_eq!(
        u64_at(ruvic_video, &["width"]),
        u64_at(ff_video, &["width"]),
        "video width differs"
    );
    assert_eq!(
        u64_at(ruvic_video, &["height"]),
        u64_at(ff_video, &["height"]),
        "video height differs"
    );

    let ruvic_audio = streams_of(&info)
        .iter()
        .find(|s| str_at(s, &["kind"]) == Some("audio"))
        .expect("ruvic audio stream");
    let ff_audio = first_stream(&reference, "audio").expect("ffprobe audio stream");
    assert_eq!(
        str_at(ruvic_audio, &["codec_name"]),
        str_at(ff_audio, &["codec_name"]),
        "audio codec differs"
    );
    assert_eq!(
        u64_at(ruvic_audio, &["sample_rate"]),
        u64_at(ff_audio, &["sample_rate"]),
        "audio sample rate differs"
    );
    assert_eq!(
        u64_at(ruvic_audio, &["channels"]),
        u64_at(ff_audio, &["channels"]),
        "audio channel count differs"
    );
}

/// A faststart remux must keep every sample: same frame counts, same codecs,
/// and the result must decode without a single ffmpeg warning.
#[test]
fn diff_remux_preserves_sample_tables() {
    let oracle = require_oracle!();
    let work = tempfile::tempdir().unwrap();
    let src = oracle.make_source(work.path(), "src_30fps.mp4", "30", "4");
    let out = work.path().join("remuxed.mp4");

    native_cmd(work.path())
        .args(["remux", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();

    oracle.decode_gate(&out);

    let before = oracle.probe(&src);
    let after = oracle.probe(&out);

    let dur_before = f64_at(&before, &["format", "duration"]).unwrap();
    let dur_after = f64_at(&after, &["format", "duration"]).unwrap();
    assert!(
        (dur_before - dur_after).abs() <= 0.02,
        "remux changed duration: {dur_before} -> {dur_after}"
    );

    for codec_type in ["video", "audio"] {
        let a = first_stream(&before, codec_type).expect("source stream");
        let b = first_stream(&after, codec_type).expect("output stream");
        assert_eq!(
            str_at(a, &["codec_name"]),
            str_at(b, &["codec_name"]),
            "{codec_type} codec changed across remux"
        );
        if let (Some(frames_a), Some(frames_b)) =
            (u64_at(a, &["nb_frames"]), u64_at(b, &["nb_frames"]))
        {
            assert_eq!(
                frames_a, frames_b,
                "{codec_type} sample count changed across remux"
            );
        }
    }
}

/// `ruvic trim` (keyframe-anchored, stream copy) vs `ffmpeg -ss/-t -c copy`.
#[test]
fn diff_trim_matches_stream_copy() {
    let oracle = require_oracle!();
    let work = tempfile::tempdir().unwrap();
    let src = oracle.make_source(work.path(), "src_30fps.mp4", "30", "6");
    let out = work.path().join("trimmed.mp4");
    let reference = work.path().join("reference.mp4");

    native_cmd(work.path())
        .args([
            "trim",
            src.to_str().unwrap(),
            "--start",
            "2",
            "--duration",
            "2",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    oracle.ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-ss",
        "2",
        "-t",
        "2",
        "-i",
        src.to_str().unwrap(),
        "-c",
        "copy",
        reference.to_str().unwrap(),
    ]);

    oracle.decode_gate(&out);

    let trimmed = oracle.probe(&out);
    let ref_clip = oracle.probe(&reference);

    let dur_trimmed = f64_at(&trimmed, &["format", "duration"]).expect("trimmed duration");
    let dur_ref = f64_at(&ref_clip, &["format", "duration"]).expect("reference duration");
    assert!(
        (dur_trimmed - dur_ref).abs() <= 0.5,
        "trim length differs from ffmpeg stream copy: ruvic {dur_trimmed}s vs ffmpeg {dur_ref}s"
    );
    assert!(
        (1.0..=3.5).contains(&dur_trimmed),
        "trim of 2s produced {dur_trimmed}s"
    );

    // Both streams survive the cut, and the clip starts at 0 (rebased timestamps).
    assert!(
        first_stream(&trimmed, "video").is_some(),
        "trim dropped the video stream"
    );
    assert!(
        first_stream(&trimmed, "audio").is_some(),
        "trim dropped the audio stream"
    );
    let start_time = f64_at(&trimmed, &["format", "start_time"]).unwrap_or(0.0);
    assert!(
        start_time.abs() <= 0.05,
        "trimmed clip does not start at 0 (start_time = {start_time})"
    );
}

/// Dual-input DASH mux: two fragmented single-track MP4s in, one standard
/// Faststart MP4 out, with both streams intact and decodable.
#[test]
fn diff_dual_mux_rebuilds_dash_streams() {
    let oracle = require_oracle!();
    let work = tempfile::tempdir().unwrap();
    let src = oracle.make_source(work.path(), "src_30fps.mp4", "30", "5");
    let (video, audio) = oracle.split_dash(work.path(), &src);
    let out = work.path().join("muxed.mp4");

    native_cmd(work.path())
        .args([
            "remux",
            "--video",
            video.to_str().unwrap(),
            "--audio",
            audio.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    oracle.decode_gate(&out);

    let muxed = oracle.probe(&out);
    let source = oracle.probe(&src);

    let video_streams = streams_of(&muxed)
        .iter()
        .filter(|s| str_at(s, &["codec_type"]) == Some("video"))
        .count();
    let audio_streams = streams_of(&muxed)
        .iter()
        .filter(|s| str_at(s, &["codec_type"]) == Some("audio"))
        .count();
    assert_eq!(
        video_streams, 1,
        "dual mux should emit exactly one video track"
    );
    assert_eq!(
        audio_streams, 1,
        "dual mux should emit exactly one audio track"
    );

    let dur_src = f64_at(&source, &["format", "duration"]).unwrap();
    let dur_mux = f64_at(&muxed, &["format", "duration"]).unwrap();
    assert!(
        (dur_src - dur_mux).abs() <= 0.15,
        "dual mux duration drift: source {dur_src}s vs muxed {dur_mux}s"
    );

    for codec_type in ["video", "audio"] {
        let a = first_stream(&source, codec_type).unwrap();
        let b = first_stream(&muxed, codec_type).unwrap();
        assert_eq!(
            str_at(a, &["codec_name"]),
            str_at(b, &["codec_name"]),
            "{codec_type} codec changed across dual mux"
        );
        if let (Some(frames_a), Some(frames_b)) =
            (u64_at(a, &["nb_frames"]), u64_at(b, &["nb_frames"]))
        {
            assert_eq!(
                frames_a, frames_b,
                "{codec_type} sample count changed across dual mux"
            );
        }
    }
}

/// 30000/1001 material is the classic `f64` timescale trap: a float round-tripped
/// `mvhd`/`mdhd` duration drifts by a frame on long assets. The i128 rescale must
/// stay within a millisecond of ffprobe's own read of the file.
#[test]
fn diff_ntsc_timescale_has_no_drift() {
    let oracle = require_oracle!();
    let work = tempfile::tempdir().unwrap();
    let src = oracle.make_source(work.path(), "src_ntsc.mp4", "30000/1001", "10");
    let out = work.path().join("ntsc_remuxed.mp4");

    native_cmd(work.path())
        .args(["remux", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();

    oracle.decode_gate(&out);

    let before = oracle.probe(&src);
    let after = oracle.probe(&out);

    let dur_before = f64_at(&before, &["format", "duration"]).unwrap();
    let dur_after = f64_at(&after, &["format", "duration"]).unwrap();
    assert!(
        (dur_before - dur_after).abs() <= 0.001,
        "NTSC duration drift beyond 1 ms: {dur_before} vs {dur_after}"
    );

    let frames_before = u64_at(first_stream(&before, "video").unwrap(), &["nb_frames"]).unwrap();
    let frames_after = u64_at(first_stream(&after, "video").unwrap(), &["nb_frames"]).unwrap();
    assert_eq!(
        frames_before, frames_after,
        "NTSC frame count changed across remux"
    );

    // The video track's own time base must survive untouched.
    assert_eq!(
        str_at(first_stream(&before, "video").unwrap(), &["time_base"]),
        str_at(first_stream(&after, "video").unwrap(), &["time_base"]),
        "video timescale changed across remux"
    );
}
