#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use tempfile::TempDir;

const FAKE_BIN_SRC: &str = r##"
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().collect();
    let exe_name = Path::new(&args[0])
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let is_ffprobe = exe_name.contains("ffprobe") || args.iter().any(|a| a == "-show_format");

    if args.iter().any(|a| a == "-version") {
        if is_ffprobe {
            println!("ffprobe version 7.1-ruvic-fake");
        } else {
            println!("ffmpeg version 7.1-ruvic-fake");
        }
        return;
    }

    if args.iter().any(|a| a == "-encoders") {
        println!("Encoders:");
        println!(" ------");
        println!(" V..... libx264              libx264 H.264 / AVC");
        println!(" V..... libx265              libx265 H.265 / HEVC");
        println!(" V..... libvpx-vp9           libvpx VP9");
        println!(" V..... libaom-av1           libaom AV1");
        if env::var("RUVIC_FAKE_HAS_NVENC").as_deref() == Ok("1") {
            println!(" V..... h264_nvenc           NVIDIA NVENC H.264 encoder");
        }
        println!(" A..... aac                  AAC (Advanced Audio Coding)");
        println!(" A..... libmp3lame           LAME MP3");
        println!(" A..... flac                 FLAC");
        println!(" A..... libopus              libopus Opus");
        return;
    }

    if args.iter().any(|a| a == "-hwaccels") {
        println!("Hardware acceleration methods:");
        if env::var("RUVIC_FAKE_HAS_NVENC").as_deref() == Ok("1") {
            println!("cuda");
        }
        return;
    }

    let mode = env::var("RUVIC_FAKE_MODE").unwrap_or_default();

    if is_ffprobe {
        let target = args.last().cloned().unwrap_or_default();
        if mode == "corrupt_output" && target.contains(".ruvic-tmp-") {
            println!(r#"{{"format":{{"format_name":"mp4"}},"streams":[]}}"#);
            return;
        }
        if target.to_ascii_lowercase().contains("vp9") {
            println!(
                r#"{{
                    "format": {{
                        "filename": {target_json},
                        "format_name": "matroska,webm",
                        "format_long_name": "Matroska / WebM",
                        "duration": "8.000",
                        "size": "2048",
                        "bit_rate": "256000"
                    }},
                    "streams": [
                        {{"index": 0, "codec_type": "video", "codec_name": "vp9", "width": 1280, "height": 720, "avg_frame_rate": "30/1"}},
                        {{"index": 1, "codec_type": "audio", "codec_name": "opus", "sample_rate": "48000", "channels": 2, "tags": {{"language": "eng"}}}}
                    ],
                    "chapters": []
                }}"#,
                target_json = serde_json_escape(&target),
            );
            return;
        }
        println!(
            r#"{{
                "format": {{
                    "filename": {target_json},
                    "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
                    "format_long_name": "QuickTime / MOV",
                    "duration": "12.500",
                    "size": "4096",
                    "bit_rate": "512000"
                }},
                "streams": [
                    {{"index": 0, "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080, "avg_frame_rate": "30/1", "pix_fmt": "yuv420p"}},
                    {{"index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2, "tags": {{"language": "und"}}}}
                ],
                "chapters": [
                    {{"id": 0, "start_time": "0.0", "end_time": "12.5", "tags": {{"title": "Intro"}}}}
                ]
            }}"#,
            target_json = serde_json_escape(&target),
        );
        return;
    }

    if let Ok(record_path) = env::var("RUVIC_FAKE_RECORD_ARGS") {
        let _ = fs::write(record_path, args[1..].join("\n"));
    }

    if let Ok(ms_str) = env::var("RUVIC_FAKE_SLEEP_MS") {
        if let Ok(ms) = ms_str.parse::<u64>() {
            thread::sleep(Duration::from_millis(ms));
        }
    }

    if mode == "fail_encoder" {
        eprintln!("Simulated encoder failure with ?token=supersecret123");
        std::process::exit(1);
    }

    println!("out_time_us=5000000");
    println!("fps=60.0");
    println!("speed=2.0x");
    println!("progress=continue");
    println!("out_time_us=12500000");
    println!("fps=60.0");
    println!("speed=2.0x");
    println!("progress=end");
    let _ = std::io::stdout().flush();

    if let Some(output_path) = args.last() {
        if mode == "empty_output" {
            fs::write(output_path, b"").expect("write empty fake output");
        } else {
            fs::write(output_path, b"FAKE_MEDIA_DATA_OK").expect("write fake output");
        }
    }
}

fn serde_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
"##;

pub struct FakeFfmpegPair {
    _dir: TempDir,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

static COMPILED_BIN_CACHE: OnceLock<PathBuf> = OnceLock::new();

fn compile_fake_bin_once() -> PathBuf {
    COMPILED_BIN_CACHE
        .get_or_init(|| {
            let dir = std::env::temp_dir().join("ruvic-fake-ffmpeg-cache");
            std::fs::create_dir_all(&dir).expect("create fake bin cache dir");
            let src_path = dir.join("fake_ffmpeg.rs");
            std::fs::write(&src_path, FAKE_BIN_SRC).expect("write fake ffmpeg source");
            let exe_name = if cfg!(windows) {
                "ruvic_fake_ffmpeg_v1.exe"
            } else {
                "ruvic_fake_ffmpeg_v1"
            };
            let out_path = dir.join(exe_name);
            let status = Command::new("rustc")
                .arg("--edition")
                .arg("2021")
                .arg(&src_path)
                .arg("-o")
                .arg(&out_path)
                .status()
                .expect("invoke rustc to build fake ffmpeg");
            assert!(status.success(), "rustc failed building fake ffmpeg");
            out_path
        })
        .clone()
}

pub fn make_fake_ffmpeg_pair() -> FakeFfmpegPair {
    let base = compile_fake_bin_once();
    let dir = tempfile::tempdir().expect("tempdir for fake binaries");
    let ffmpeg = dir.path().join(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    });
    let ffprobe = dir.path().join(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    std::fs::copy(&base, &ffmpeg).expect("copy fake ffmpeg");
    std::fs::copy(&base, &ffprobe).expect("copy fake ffprobe");
    FakeFfmpegPair {
        _dir: dir,
        ffmpeg,
        ffprobe,
    }
}

pub fn ruvic_cmd(isolated_dir: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("ruvic");
    cmd.env("RUVI_CONFIG_DIR", isolated_dir.join("cfg"))
        .env("RUVI_CACHE_DIR", isolated_dir.join("cache"))
        .env("RUVI_STATE_DIR", isolated_dir.join("state"));
    cmd
}

pub mod synthetic_media;
