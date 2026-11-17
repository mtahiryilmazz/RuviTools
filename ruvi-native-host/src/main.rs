use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
const DEFAULT_JOBS: usize = 4;
const MIN_JOBS: usize = 1;
const MAX_JOBS: usize = 16;

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
    jobs: Option<usize>,
    #[serde(default)]
    clip: Option<ClipSpec>,
}

#[derive(Debug, Clone)]
struct HostRuntimeConfig {
    download_dir: PathBuf,
    staging_dir: PathBuf,
    allow_private_net: bool,
    allow_local_test_url: bool,
}

impl Default for HostRuntimeConfig {
    fn default() -> Self {
        Self {
            download_dir: download_root(),
            staging_dir: staging_root(),
            allow_private_net: false,
            allow_local_test_url: false,
        }
    }
}

fn resolve_jobs(jobs: Option<usize>) -> usize {
    jobs.unwrap_or(DEFAULT_JOBS).clamp(MIN_JOBS, MAX_JOBS)
}

fn write_native_frame<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame exceeds u32::MAX"))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

fn read_native_frame<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > MAX_MESSAGE_BYTES {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(buf))
}

fn send_native_message<W: Write>(writer_lock: &Arc<Mutex<W>>, value: &Value) {
    if let Ok(mut out) = writer_lock.lock() {
        let _ = write_native_frame(&mut *out, value);
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

fn is_allowed_request_url(url: &str, allow_local_test_url: bool) -> bool {
    if is_allowed_youtube_url(url) {
        return true;
    }
    if allow_local_test_url
        && !url.is_empty()
        && url.len() <= 2048
        && let Some(rest) = url.strip_prefix("http://127.0.0.1:")
    {
        return rest.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '=' | '&' | '%' | '.' | ':' | '+' | '/' | '?')
        });
    }
    false
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
    handle_download_job_with_config(msg, out, HostRuntimeConfig::default()).await;
}

async fn handle_download_job_with_config<W: Write>(
    msg: IncomingMessage,
    out: Arc<Mutex<W>>,
    cfg: HostRuntimeConfig,
) {
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

    if !is_allowed_request_url(&msg.url, cfg.allow_local_test_url) {
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

    let jobs = resolve_jobs(msg.jobs);

    send_native_message(
        &out,
        &json!({
            "jobId": job_id,
            "percent": 15,
            "jobs": jobs,
            "status": format!("ruvid saf Rust motoru ile paralel indiriliyor (--jobs {jobs})...")
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

    let job_staging = cfg.staging_dir.join(&job_id);
    let _ = std::fs::create_dir_all(&job_staging);
    let final_dir = cfg.download_dir;
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
        jobs,
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
        allow_private_net: cfg.allow_private_net,
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
            "jobs": jobs,
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

fn handle_sync_message<W: Write>(msg: &IncomingMessage, out: &Arc<Mutex<W>>) -> bool {
    match msg.action.as_str() {
        "ping" => {
            let resp = PingResponse {
                ok: true,
                version: "2.0.0",
                engine: "ruvid+ruvic-rust",
                port: 0,
            };
            if let Ok(v) = serde_json::to_value(resp) {
                send_native_message(out, &v);
            }
            true
        }
        "open_file" => {
            match validate_download_path(&msg.file_path) {
                Ok(valid_path) => {
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(valid_path.as_os_str())
                        .spawn();
                    send_native_message(out, &json!({"ok": true}));
                }
                Err(e) => {
                    send_native_message(out, &json!({"ok": false, "error": e}));
                }
            }
            true
        }
        "open_folder" => {
            let folder = download_root();
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::process::Command::new("explorer.exe")
                .arg(folder.as_os_str())
                .current_dir(Path::new("C:\\Windows"))
                .spawn();
            send_native_message(out, &json!({"ok": true}));
            true
        }
        "download" => false,
        other => {
            send_native_message(
                out,
                &json!({
                    "jobId": msg.job_id,
                    "done": true,
                    "ok": false,
                    "error": format!("Bilinmeyen işlem: {other}")
                }),
            );
            true
        }
    }
}

fn process_native_stream_sync<R: Read, W: Write>(
    mut reader: R,
    out: Arc<Mutex<W>>,
    mut on_download: impl FnMut(IncomingMessage, Arc<Mutex<W>>),
) {
    while let Ok(Some(buf)) = read_native_frame(&mut reader) {
        let parsed: Result<IncomingMessage, _> = serde_json::from_slice(&buf);
        let Ok(msg) = parsed else {
            send_native_message(
                &out,
                &json!({
                    "jobId": "",
                    "done": true,
                    "ok": false,
                    "error": "Geçersiz JSON mesajı."
                }),
            );
            continue;
        };

        if !handle_sync_message(&msg, &out) {
            on_download(msg, out.clone());
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GeTube NativeHost 2.0.0 (ruvid+ruvic pure-Rust in-process)");
        return;
    }

    let stdout_lock = Arc::new(Mutex::new(io::stdout()));
    let stdin = io::stdin();
    process_native_stream_sync(stdin, stdout_lock, |msg, out_clone| {
        tokio::spawn(async move {
            handle_download_job(msg, out_clone).await;
        });
    });
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

    #[test]
    fn test_resolve_jobs_defaults_and_clamps() {
        assert_eq!(resolve_jobs(None), 4);
        assert_eq!(resolve_jobs(Some(0)), 1);
        assert_eq!(resolve_jobs(Some(1)), 1);
        assert_eq!(resolve_jobs(Some(2)), 2);
        assert_eq!(resolve_jobs(Some(4)), 4);
        assert_eq!(resolve_jobs(Some(8)), 8);
        assert_eq!(resolve_jobs(Some(16)), 16);
        assert_eq!(resolve_jobs(Some(64)), 16);
    }

    #[test]
    fn test_incoming_message_parses_jobs_variants() {
        let omitted: IncomingMessage = serde_json::from_str(
            r#"{"action":"download","jobId":"job-1","url":"https://www.youtube.com/watch?v=jNQXAC9IVRw"}"#,
        )
        .expect("parse without jobs");
        assert_eq!(resolve_jobs(omitted.jobs), 4);

        let explicit: IncomingMessage = serde_json::from_str(
            r#"{"action":"download","jobId":"job-2","jobs":8,"url":"https://www.youtube.com/watch?v=jNQXAC9IVRw"}"#,
        )
        .expect("parse jobs=8");
        assert_eq!(explicit.jobs, Some(8));
        assert_eq!(resolve_jobs(explicit.jobs), 8);

        let zero: IncomingMessage =
            serde_json::from_str(r#"{"action":"download","jobs":0}"#).expect("parse jobs=0");
        assert_eq!(resolve_jobs(zero.jobs), 1);

        let oversized: IncomingMessage =
            serde_json::from_str(r#"{"action":"download","jobs":64}"#).expect("parse jobs=64");
        assert_eq!(resolve_jobs(oversized.jobs), 16);

        let null_jobs: IncomingMessage =
            serde_json::from_str(r#"{"action":"download","jobs":null}"#).expect("parse jobs=null");
        assert_eq!(resolve_jobs(null_jobs.jobs), 4);
    }

    #[test]
    fn test_is_allowed_youtube_url_accepts_valid_and_rejects_invalid() {
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/watch?v=jNQXAC9IVRw"
        ));
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/watch?v=jNQXAC9IVRw&t=12s"
        ));

        assert!(!is_allowed_youtube_url(""));
        assert!(!is_allowed_youtube_url(
            "http://www.youtube.com/watch?v=jNQXAC9IVRw"
        ));
        assert!(!is_allowed_youtube_url("https://evil.example/watch?v=123"));
        assert!(!is_allowed_youtube_url(
            "https://www.youtube.com/watch?v=abc def"
        ));
        let over_limit = format!("https://www.youtube.com/watch?v={}", "a".repeat(2040));
        assert!(!is_allowed_youtube_url(&over_limit));
    }

    #[test]
    fn test_is_valid_job_id_cases() {
        assert!(is_valid_job_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_job_id("job123"));

        assert!(!is_valid_job_id(""));
        assert!(!is_valid_job_id("../escape"));
        assert!(!is_valid_job_id("job_with_underscore"));
        assert!(!is_valid_job_id(&"a".repeat(65)));
    }

    #[test]
    fn test_quality_tag_video_and_mp3() {
        assert_eq!(quality_tag("MP3", "320kbps"), "320kbps");
        assert_eq!(quality_tag("MP3", "192kbps"), "192kbps");
        assert_eq!(quality_tag("MP3", "other"), "192kbps");
        assert_eq!(quality_tag("Video", ""), "En iyi");
        assert_eq!(quality_tag("Video", "best"), "En iyi");
        assert_eq!(quality_tag("Video", "1080p"), "1080p");
    }

    fn encode_frame(bytes: &[u8]) -> Vec<u8> {
        let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(bytes);
        frame
    }

    fn decode_frames(raw: &[u8]) -> Vec<Value> {
        let mut cursor = io::Cursor::new(raw);
        let mut out = Vec::new();
        while let Ok(Some(bytes)) = read_native_frame(&mut cursor) {
            let val: Value = serde_json::from_slice(&bytes).expect("valid JSON frame");
            out.push(val);
        }
        out
    }

    #[test]
    fn test_native_framing_round_trip_ping_invalid_json_and_unknown_action() {
        let mut input = Vec::new();
        input.extend_from_slice(&encode_frame(br#"{"action":"ping"}"#));
        input.extend_from_slice(&encode_frame(b"{not-valid-json"));
        input.extend_from_slice(&encode_frame(
            br#"{"action":"unknown_op","jobId":"job-99"}"#,
        ));

        let out_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let mut download_calls = 0usize;
        process_native_stream_sync(io::Cursor::new(input), out_buf.clone(), |_, _| {
            download_calls += 1;
        });
        assert_eq!(download_calls, 0);

        let raw_out = out_buf.lock().expect("lock output").clone();
        let responses = decode_frames(&raw_out);
        assert_eq!(responses.len(), 3);

        assert_eq!(responses[0]["ok"], json!(true));
        assert_eq!(responses[0]["version"], json!("2.0.0"));
        assert_eq!(responses[0]["engine"], json!("ruvid+ruvic-rust"));

        assert_eq!(responses[1]["ok"], json!(false));
        assert_eq!(responses[1]["done"], json!(true));

        assert_eq!(responses[2]["ok"], json!(false));
        assert_eq!(responses[2]["done"], json!(true));
        assert_eq!(responses[2]["jobId"], json!("job-99"));
    }

    #[test]
    fn test_native_framing_stops_on_zero_and_oversized_frames() {
        let zero_frame = 0u32.to_le_bytes().to_vec();
        assert!(
            read_native_frame(&mut io::Cursor::new(zero_frame))
                .expect("read zero frame")
                .is_none()
        );

        let oversized_frame = ((MAX_MESSAGE_BYTES as u32) + 1).to_le_bytes().to_vec();
        assert!(
            read_native_frame(&mut io::Cursor::new(oversized_frame))
                .expect("read oversized frame")
                .is_none()
        );
    }

    struct TempTestDir {
        path: PathBuf,
    }

    impl TempTestDir {
        fn new(prefix: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ruvi_native_host_test_{prefix}_{}_{}",
                std::process::id(),
                n
            ));
            let _ = std::fs::create_dir_all(&path);
            Self { path }
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn test_handle_download_job_end_to_end_parallel_jobs_range_mock() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // 128 KiB payload -> with jobs=8, ruvid plan_chunks splits into 16 x 8 KiB HTTP Range chunks.
        let payload: Vec<u8> = (0..(128 * 1024))
            .map(|i| ((i * 31 + 7) % 251) as u8)
            .collect();
        let range_requests = Arc::new(AtomicUsize::new(0));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local mock server");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://127.0.0.1:{}", addr.port());

        let html = format!(
            r#"<!doctype html>
<html>
<head><title>Mock NativeHost Video</title></head>
<body>
<script>
var ytInitialPlayerResponse = {{
  "playabilityStatus": {{ "status": "OK" }},
  "videoDetails": {{
    "videoId": "MOCKJOBS01",
    "title": "NativeHost Parallel Jobs Test"
  }},
  "streamingData": {{
    "formats": [
      {{
        "itag": 22,
        "url": "{base}/media/720.mp4",
        "mimeType": "video/mp4; codecs=\"avc1.64001F, mp4a.40.2\"",
        "width": 1280,
        "height": 720,
        "bitrate": 1500000,
        "fps": 30,
        "contentLength": "{len}"
      }}
    ]
  }}
}};
</script>
</body>
</html>"#,
            base = base_url,
            len = payload.len()
        );

        let server_payload = Arc::new(payload.clone());
        let server_html = Arc::new(html);
        let server_ranges = Arc::clone(&range_requests);

        let server_task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let payload = Arc::clone(&server_payload);
                let html = Arc::clone(&server_html);
                let ranges = Arc::clone(&server_ranges);
                tokio::spawn(async move {
                    let mut buf = Vec::with_capacity(2048);
                    let mut tmp = [0u8; 1024];
                    loop {
                        let Ok(n) = stream.read(&mut tmp).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }

                    let req_text = String::from_utf8_lossy(&buf);
                    let mut lines = req_text.lines();
                    let first_line = lines.next().unwrap_or("");
                    let mut parts = first_line.split_whitespace();
                    let method = parts.next().unwrap_or("GET");
                    let path = parts.next().unwrap_or("/");

                    let mut range_hdr: Option<String> = None;
                    for line in lines {
                        if let Some((k, v)) = line.split_once(':')
                            && k.trim().eq_ignore_ascii_case("range")
                        {
                            range_hdr = Some(v.trim().to_string());
                        }
                    }

                    if path.starts_with("/watch") {
                        let body = html.as_bytes();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        if method != "HEAD" {
                            let _ = stream.write_all(body).await;
                        }
                        return;
                    }

                    if path.starts_with("/media/720.mp4") {
                        let total = payload.len();
                        if let Some(r) = range_hdr
                            && let Some(spec) = r.strip_prefix("bytes=")
                            && let Some((s_str, e_str)) = spec.split_once('-')
                            && let Ok(start) = s_str.trim().parse::<usize>()
                        {
                            let end = e_str
                                .trim()
                                .parse::<usize>()
                                .unwrap_or(total - 1)
                                .min(total - 1);
                            if start <= end && end < total {
                                ranges.fetch_add(1, Ordering::Relaxed);
                                let slice = &payload[start..=end];
                                let resp = format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    start,
                                    end,
                                    total,
                                    slice.len()
                                );
                                let _ = stream.write_all(resp.as_bytes()).await;
                                if method != "HEAD" {
                                    let _ = stream.write_all(slice).await;
                                }
                                return;
                            }
                        }

                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            total
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        if method != "HEAD" {
                            let _ = stream.write_all(&payload).await;
                        }
                    }
                });
            }
        });

        let tmp = TempTestDir::new("mock_jobs8");
        let cfg = HostRuntimeConfig {
            download_dir: tmp.path.join("downloads"),
            staging_dir: tmp.path.join("staging"),
            allow_private_net: true,
            allow_local_test_url: true,
        };

        let msg = IncomingMessage {
            action: "download".to_string(),
            job_id: "job-parallel-8".to_string(),
            media_type: "Video".to_string(),
            quality: "720p".to_string(),
            url: format!("{base_url}/watch?v=MOCKJOBS01"),
            file_path: String::new(),
            jobs: Some(8),
            clip: None,
        };

        let out_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        handle_download_job_with_config(msg, out_buf.clone(), cfg.clone()).await;
        server_task.abort();

        let raw_out = out_buf.lock().expect("lock out").clone();
        let frames = decode_frames(&raw_out);
        assert!(
            frames.len() >= 2,
            "expected initial progress and completion frames, got: {frames:?}"
        );

        assert_eq!(frames[0]["jobs"], json!(8));
        assert!(
            frames[0]["status"]
                .as_str()
                .unwrap_or("")
                .contains("--jobs 8"),
            "expected status to contain --jobs 8, got: {}",
            frames[0]["status"]
        );

        let final_frame = frames.last().expect("final frame");
        assert_eq!(final_frame["done"], json!(true));
        assert_eq!(final_frame["ok"], json!(true));
        assert_eq!(final_frame["percent"], json!(100));
        assert_eq!(final_frame["jobs"], json!(8));
        assert_eq!(final_frame["fileSize"], json!(payload.len() as u64));

        let file_path = PathBuf::from(
            final_frame["filePath"]
                .as_str()
                .expect("filePath in final frame"),
        );
        assert!(file_path.starts_with(&cfg.download_dir));
        let downloaded = std::fs::read(&file_path).expect("read downloaded file");
        assert_eq!(downloaded, payload);

        // jobs=8 on 128 KiB triggers 16 parallel Range chunks in ruvid::downloader::plan_chunks.
        assert!(
            range_requests.load(Ordering::Relaxed) >= 8,
            "expected multiple HTTP Range requests for --jobs 8, got {}",
            range_requests.load(Ordering::Relaxed)
        );
    }
}
