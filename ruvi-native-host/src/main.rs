use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ruvic::backend::native::NativeMediaBackend;
use ruvic::config::RuvicConfig;
use ruvic::operations::extract_audio::{ExtractAudioRequest, execute_extract_audio};
use ruvic::operations::trim::{TrimRequest, execute_trim};
use ruvid::config::RuvidConfig;
use ruvid::domain::download_job::DownloadJob;
use ruvid::downloader::execute_download;
use ruvid::storage::sanitize_filename;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
struct ClipSpec {
    #[serde(rename = "startSec")]
    start_sec: f64,
    #[serde(rename = "endSec")]
    end_sec: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct IncomingMessage {
    #[serde(default)]
    action: String,
    #[serde(default, rename = "jobId")]
    job_id: String,
    #[serde(default, rename = "type")]
    media_type: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "filePath")]
    file_path: String,
    #[serde(default)]
    clip: Option<ClipSpec>,
}

fn send_native_message(stdout_lock: &Arc<Mutex<io::Stdout>>, value: &Value) {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return;
    };
    let len = bytes.len() as u32;
    let header = len.to_le_bytes();
    if let Ok(mut out) = stdout_lock.lock() {
        let _ = out.write_all(&header);
        let _ = out.write_all(&bytes);
        let _ = out.flush();
    }
}

fn download_root() -> PathBuf {
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    profile.join("Downloads").join("GeTube")
}

fn staging_root() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    local.join("GeTube").join("staging")
}

fn is_allowed_youtube_url(url: &str) -> bool {
    if url.is_empty() || url.len() > 2048 {
        return false;
    }
    url.starts_with("https://www.youtube.com/watch?")
        && url[30..].chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=' | '&' | '%' | '.' | ':' | '+')
        })
}

fn is_valid_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.len() <= 64
        && job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn validate_clip(clip: &ClipSpec) -> Result<(), &'static str> {
    if !clip.start_sec.is_finite() || !clip.end_sec.is_finite() {
        return Err("Kesit zamanı geçersiz.");
    }
    if clip.start_sec < 0.0 || clip.end_sec <= clip.start_sec {
        return Err("Kesit bitişi başlangıçtan sonra olmalı.");
    }
    Ok(())
}

fn file_stamp(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{h:02}-{m:02}-{s:02}")
}

fn quality_tag(media_type: &str, quality: &str) -> String {
    if media_type.eq_ignore_ascii_case("MP3") {
        if quality == "320kbps" {
            "320kbps".to_string()
        } else {
            "192kbps".to_string()
        }
    } else if quality.is_empty() || quality == "best" {
        "En iyi".to_string()
    } else {
        quality.to_string()
    }
}

fn validate_download_path(path_str: &str) -> Result<PathBuf, String> {
    if path_str.trim().is_empty() {
        return Err("Dosya yolu boş.".to_string());
    }
    let root = download_root();
    let _ = std::fs::create_dir_all(&root);
    let canon_root = std::fs::canonicalize(&root).unwrap_or(root);
    let canon_target =
        std::fs::canonicalize(path_str).map_err(|_| "İndirilen dosya bulunamadı.".to_string())?;
    if !canon_target.starts_with(&canon_root) {
        return Err("Yalnızca GeTube indirme klasöründeki dosyalar açılabilir.".to_string());
    }
    Ok(canon_target)
}

async fn handle_download_job(msg: IncomingMessage, out: Arc<Mutex<io::Stdout>>) {
    let job_id = msg.job_id.clone();
    if !is_valid_job_id(&job_id) {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 0,
                "done": true,
                "ok": false,
                "status": "İşlem başarısız",
                "error": "Geçersiz iş kimliği."
            }),
        );
        return;
    }

    if !is_allowed_youtube_url(&msg.url) {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 0,
                "done": true,
                "ok": false,
                "status": "İşlem başarısız",
                "error": "Geçersiz YouTube adresi."
            }),
        );
        return;
    }

    if let Some(ref c) = msg.clip
        && let Err(err_msg) = validate_clip(c)
    {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 0,
                "done": true,
                "ok": false,
                "status": "İşlem başarısız",
                "error": err_msg
            }),
        );
        return;
    }

    send_native_message(
        &out,
        &json!({
            "jobId": job_id,
            "percent": 15,
            "status": "ruvid saf Rust motoru ile paralel indiriliyor (--jobs 4)..."
        }),
    );

    let is_audio = msg.media_type.eq_ignore_ascii_case("MP3");
    let tag = quality_tag(&msg.media_type, &msg.quality);
    let final_tag = match msg.clip {
        Some(ref c) => format!(
            "clip {}--{}",
            file_stamp(c.start_sec),
            file_stamp(c.end_sec)
        ),
        None => tag,
    };

    let job_staging = staging_root().join(&job_id);
    let _ = std::fs::create_dir_all(&job_staging);
    let final_dir = download_root();
    let _ = std::fs::create_dir_all(&final_dir);

    let format_spec = if is_audio {
        "bestaudio".to_string()
    } else {
        match msg.quality.as_str() {
            "1080p" | "720p" | "480p" => msg.quality.clone(),
            _ => "best".to_string(),
        }
    };

    let raw_template = job_staging.join("%(title)s.%(ext)s");
    let ruvid_cfg = RuvidConfig::default();
    let dl_job = DownloadJob {
        url: msg.url.clone(),
        output_template: Some(raw_template),
        format_spec,
        jobs: 4,
        resume: true,
        overwrite: true,
        limit_rate: None,
        retry_count: 3,
        timeout_seconds: 30,
        proxy: None,
        headers: Vec::new(),
        write_manifest: None,
        to_stdout: false,
        subtitles: false,
        subtitles_lang: None,
        sub_format: "vtt".to_string(),
        auto_subs: false,
        audio_lang: None,
        split_chapters: false,
        clip: None,
        start_time: None,
        end_time: None,
        duration_seconds: None,
        allow_private_net: false,
    };

    let outcome = match execute_download(&ruvid_cfg, dl_job, false).await {
        Ok(o) => o,
        Err(e) => {
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "percent": 0,
                    "done": true,
                    "ok": false,
                    "status": "İndirme başarısız",
                    "error": e.to_string()
                }),
            );
            let _ = std::fs::remove_dir_all(&job_staging);
            return;
        }
    };

    let mut working_path = PathBuf::from(&outcome.output_path);
    let mut clean_title = sanitize_filename(&outcome.title);
    if clean_title.chars().count() > 100 {
        clean_title = clean_title.chars().take(100).collect();
    }

    let backend = NativeMediaBackend::new();
    let ruvic_cfg = RuvicConfig::default();

    if let Some(ref c) = msg.clip {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 75,
                "status": "ruvic saf Rust medya motoru ile kesit kırpılıyor..."
            }),
        );
        let trimmed_path = job_staging.join(format!("{clean_title} [{final_tag}].mp4"));
        let trim_req = TrimRequest {
            input: working_path.clone(),
            output: trimmed_path.clone(),
            start: format!("{:.3}", c.start_sec),
            end: None,
            duration: Some((c.end_sec - c.start_sec).max(0.1)),
            reencode: false,
            overwrite: true,
        };
        if let Err(e) = execute_trim(&backend, &ruvic_cfg, trim_req, false).await {
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "percent": 0,
                    "done": true,
                    "ok": false,
                    "status": "Kesit hatası",
                    "error": e.to_string()
                }),
            );
            let _ = std::fs::remove_dir_all(&job_staging);
            return;
        }
        let _ = std::fs::remove_file(&working_path);
        working_path = trimmed_path;
    }

    let final_ext = if is_audio { "mp3" } else { "mp4" };
    let final_file_name = format!("{clean_title} [{final_tag}].{final_ext}");
    let final_dest = final_dir.join(&final_file_name);

    if is_audio {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 90,
                "status": "ruvic saf Rust medya motoru ile MP3'e dönüştürülüyor..."
            }),
        );
        let audio_req = ExtractAudioRequest {
            input: working_path.clone(),
            output: final_dest.clone(),
            to: Some("mp3".to_string()),
            copy: false,
            overwrite: true,
            audio_stream: None,
        };
        if let Err(e) = execute_extract_audio(&backend, &ruvic_cfg, audio_req, false).await {
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "percent": 0,
                    "done": true,
                    "ok": false,
                    "status": "MP3 dönüşüm hatası",
                    "error": e.to_string()
                }),
            );
            let _ = std::fs::remove_dir_all(&job_staging);
            return;
        }
        let _ = std::fs::remove_file(&working_path);
    } else {
        if final_dest.exists() {
            let _ = std::fs::remove_file(&final_dest);
        }
        if std::fs::rename(&working_path, &final_dest).is_err() {
            if let Err(e) = std::fs::copy(&working_path, &final_dest) {
                send_native_message(
                    &out,
                    &json!({
                        "jobId": job_id,
                        "percent": 0,
                        "done": true,
                        "ok": false,
                        "status": "Dosya taşıma hatası",
                        "error": e.to_string()
                    }),
                );
                let _ = std::fs::remove_dir_all(&job_staging);
                return;
            }
            let _ = std::fs::remove_file(&working_path);
        }
    }

    let _ = std::fs::remove_dir_all(&job_staging);
    let file_size = std::fs::metadata(&final_dest).map(|m| m.len()).unwrap_or(0);

    send_native_message(
        &out,
        &json!({
            "jobId": job_id,
            "percent": 100,
            "done": true,
            "ok": true,
            "status": "Tamamlandı",
            "fileName": final_file_name,
            "filePath": final_dest.to_string_lossy(),
            "fileSize": file_size,
            "isClip": msg.clip.is_some()
        }),
    );
}

#[derive(Serialize)]
struct PingResponse {
    ok: bool,
    version: &'static str,
    engine: &'static str,
    port: u16,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GeTube NativeHost 2.0.0 (ruvid+ruvic pure-Rust in-process)");
        return;
    }

    let stdout_lock = Arc::new(Mutex::new(io::stdout()));
    let mut stdin = io::stdin();
    let mut header = [0u8; 4];

    loop {
        if stdin.read_exact(&mut header).is_err() {
            break;
        }
        let len = u32::from_le_bytes(header) as usize;
        if len == 0 || len > MAX_MESSAGE_BYTES {
            break;
        }
        let mut buf = vec![0u8; len];
        if stdin.read_exact(&mut buf).is_err() {
            break;
        }

        let parsed: Result<IncomingMessage, _> = serde_json::from_slice(&buf);
        let Ok(msg) = parsed else {
            send_native_message(
                &stdout_lock,
                &json!({
                    "jobId": "",
                    "done": true,
                    "ok": false,
                    "error": "Geçersiz JSON mesajı."
                }),
            );
            continue;
        };

        match msg.action.as_str() {
            "ping" => {
                let resp = PingResponse {
                    ok: true,
                    version: "2.0.0",
                    engine: "ruvid+ruvic-rust",
                    port: 0,
                };
                if let Ok(v) = serde_json::to_value(resp) {
                    send_native_message(&stdout_lock, &v);
                }
            }
            "open_file" => match validate_download_path(&msg.file_path) {
                Ok(valid_path) => {
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(valid_path.as_os_str())
                        .spawn();
                    send_native_message(&stdout_lock, &json!({"ok": true}));
                }
                Err(e) => {
                    send_native_message(&stdout_lock, &json!({"ok": false, "error": e}));
                }
            },
            "open_folder" => {
                let folder = download_root();
                let _ = std::fs::create_dir_all(&folder);
                let _ = std::process::Command::new("explorer.exe")
                    .arg(folder.as_os_str())
                    .current_dir(Path::new("C:\\Windows"))
                    .spawn();
                send_native_message(&stdout_lock, &json!({"ok": true}));
            }
            "download" => {
                let out_clone = stdout_lock.clone();
                tokio::spawn(async move {
                    handle_download_job(msg, out_clone).await;
                });
            }
            other => {
                send_native_message(
                    &stdout_lock,
                    &json!({
                        "jobId": msg.job_id,
                        "done": true,
                        "ok": false,
                        "error": format!("Bilinmeyen işlem: {other}")
                    }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_clip_normal() {
        let clip = ClipSpec {
            start_sec: 10.0,
            end_sec: 30.0,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_long_duration_over_6h() {
        // Benchmark video AKeUssuu3Is duration: 36738.125s (10h 12m 18s)
        let clip = ClipSpec {
            start_sec: 0.0,
            end_sec: 36738.125,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_over_24h() {
        // Stream clipping beyond 24h
        let clip = ClipSpec {
            start_sec: 86500.0,
            end_sec: 86600.0,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_ultra_long_100h() {
        let clip = ClipSpec {
            start_sec: 0.0,
            end_sec: 360000.0,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_invalid_negative_start() {
        let clip = ClipSpec {
            start_sec: -1.0,
            end_sec: 10.0,
        };
        assert_eq!(
            validate_clip(&clip),
            Err("Kesit bitişi başlangıçtan sonra olmalı.")
        );
    }

    #[test]
    fn test_validate_clip_invalid_end_before_start() {
        let clip = ClipSpec {
            start_sec: 20.0,
            end_sec: 10.0,
        };
        assert_eq!(
            validate_clip(&clip),
            Err("Kesit bitişi başlangıçtan sonra olmalı.")
        );
    }

    #[test]
    fn test_validate_clip_invalid_equal_start_end() {
        let clip = ClipSpec {
            start_sec: 10.0,
            end_sec: 10.0,
        };
        assert_eq!(
            validate_clip(&clip),
            Err("Kesit bitişi başlangıçtan sonra olmalı.")
        );
    }

    #[test]
    fn test_validate_clip_invalid_nan() {
        let clip = ClipSpec {
            start_sec: f64::NAN,
            end_sec: 10.0,
        };
        assert_eq!(validate_clip(&clip), Err("Kesit zamanı geçersiz."));
    }

    #[test]
    fn test_validate_clip_invalid_infinite() {
        let clip = ClipSpec {
            start_sec: 0.0,
            end_sec: f64::INFINITY,
        };
        assert_eq!(validate_clip(&clip), Err("Kesit zamanı geçersiz."));
    }
}
