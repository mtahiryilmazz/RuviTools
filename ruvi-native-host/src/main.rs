use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use ruvic::backend::{MediaBackend, ProgressSink, ProgressUpdate};
use ruvic::backend::native::NativeMediaBackend;
use ruvic::backend::native::audio_transcode::{
    parse_smart_title_metadata, write_id3v23_to_mp3_file,
};
use ruvic::backend::native::dual_mux::inject_fmp4_itunes_metadata_file;
use ruvic::config::RuvicConfig;
use ruvic::operations::extract_audio::{ExtractAudioRequest, execute_extract_audio_with_sink};
use ruvic::operations::remux::{RemuxRequest, execute_remux};
use ruvic::operations::trim::{TrimRequest, execute_trim};
use ruvid::config::RuvidConfig;
use ruvid::domain::download_job::DownloadJob;
use ruvid::downloader::execute_download_with_progress;
use ruvid::resolver::ResolveContext;
use ruvid::resolver::registry::ResolverRegistry;
use ruvid::resolver::registry::fetch_playlist_entries;
use ruvid::storage::sanitize_filename;
use ruvid::telemetry::ProgressCallback;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_JOBS: usize = 4;
const MIN_JOBS: usize = 1;
const MAX_JOBS: usize = 16;

struct NativeProgressSink<W> {
    out: Arc<Mutex<W>>,
    job_id: String,
    total_duration_us: Option<u64>,
    label: &'static str,
}

impl<W: Write + Send + 'static> ProgressSink for NativeProgressSink<W> {
    fn on_progress(&self, update: ProgressUpdate) {
        let elapsed_us = update.out_time_us.unwrap_or(0);
        let total_us = self.total_duration_us.unwrap_or(0);
        let percent = if update.finished {
            98
        } else if total_us > 0 {
            (88 + ((elapsed_us.min(total_us) as f64 / total_us as f64) * 10.0).floor() as u64)
                .min(97)
        } else {
            89
        };
        let status = if total_us > 0 {
            format!(
                "{}: {} / {}",
                self.label,
                format_media_time(elapsed_us),
                format_media_time(total_us)
            )
        } else if elapsed_us > 0 {
            format!("{}: {}", self.label, format_media_time(elapsed_us))
        } else {
            format!("{}...", self.label)
        };
        send_native_message(
            &self.out,
            &json!({
                "jobId": self.job_id,
                "percent": percent,
                "status": status,
                "speed": update.speed.clone().unwrap_or_default()
            }),
        );
    }
}

fn format_media_time(microseconds: u64) -> String {
    let seconds = microseconds / 1_000_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[derive(Debug, Clone, Deserialize)]
struct ClipSpec {
    #[serde(rename = "startSec")]
    start_sec: f64,
    #[serde(rename = "endSec")]
    end_sec: f64,
    #[serde(default, rename = "chapterTitle")]
    chapter_title: Option<String>,
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
    #[serde(default)]
    channel: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    version: String,
    #[serde(default, rename = "audioLang")]
    audio_lang: String,
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

fn log_path() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    local.join("RuviTube").join("nativehost.log")
}

fn format_utc_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hh = rem / 3_600;
    let mm = (rem % 3_600) / 60;
    let ss = rem % 60;

    // Howard Hinnant's civil-from-days algorithm
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

fn redact_query_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for key in ["sig=", "lsig=", "signature="] {
        while let Some(pos) = out.find(key) {
            let val_start = pos + key.len();
            let rest = &out[val_start..];
            if rest.starts_with("REDACTED") {
                break;
            }
            let val_end = rest
                .find(['&', ' ', '"', '\n', '\r'])
                .map(|i| val_start + i)
                .unwrap_or(out.len());
            out.replace_range(val_start..val_end, "REDACTED");
        }
    }
    out
}

fn log_event(event: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > MAX_LOG_BYTES
    {
        let _ = std::fs::write(&path, b"");
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let line = format!(
            "{} {}\n",
            format_utc_timestamp(),
            redact_query_secrets(event)
        );
        let _ = f.write_all(line.as_bytes());
    }
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
    profile.join("Downloads").join("RuviTube")
}

fn legacy_download_root() -> PathBuf {
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    profile.join("Downloads").join("GeTube")
}

fn staging_root() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    local.join("RuviTube").join("staging")
}

fn is_allowed_youtube_url(url: &str) -> bool {
    if url.is_empty() || url.len() > 2048 {
        return false;
    }
    let without_frag = url.split('#').next().unwrap_or(url);
    let allowed_prefixes = [
        "https://www.youtube.com/playlist?",
        "https://m.youtube.com/playlist?",
        "https://music.youtube.com/playlist?",
        "https://www.youtube.com/watch?",
        "https://m.youtube.com/watch?",
        "https://music.youtube.com/watch?",
        "https://www.youtube.com/shorts/",
        "https://m.youtube.com/shorts/",
        "https://www.youtube.com/live/",
        "https://www.youtube.com/embed/",
        "https://youtu.be/",
    ];
    let Some(rest) = allowed_prefixes
        .iter()
        .find_map(|pfx| without_frag.strip_prefix(pfx))
    else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    rest.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '_' | '-' | '=' | '&' | '%' | '.' | ':' | '+' | '/' | '?')
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

fn normalize_mp3_bitrate_arg(quality: &str) -> String {
    let lower = quality.trim().to_ascii_lowercase();
    if lower == "vbr" || lower == "mp3:0" || lower == "0" {
        return "vbr".to_string();
    }
    if lower.starts_with("320") {
        "320k".to_string()
    } else if lower.starts_with("256") {
        "256k".to_string()
    } else if lower.starts_with("128") {
        "128k".to_string()
    } else {
        "192k".to_string()
    }
}

fn quality_tag(media_type: &str, quality: &str) -> String {
    if media_type.eq_ignore_ascii_case("MP3") {
        let lower = quality.trim().to_ascii_lowercase();
        if lower == "vbr" || lower == "mp3:0" {
            "VBR".to_string()
        } else if lower.starts_with("320") {
            "320kbps".to_string()
        } else if lower.starts_with("256") {
            "256kbps".to_string()
        } else if lower.starts_with("128") {
            "128kbps".to_string()
        } else {
            "192kbps".to_string()
        }
    } else if media_type.eq_ignore_ascii_case("M4A") {
        "M4A".to_string()
    } else if media_type.eq_ignore_ascii_case("OPUS")
        || media_type.eq_ignore_ascii_case("WEBM-AUDIO")
    {
        "Opus".to_string()
    } else if media_type.eq_ignore_ascii_case("WAV") {
        "WAV".to_string()
    } else if media_type.eq_ignore_ascii_case("AAC") {
        "AAC".to_string()
    } else if quality.is_empty() || quality == "best" {
        "En iyi".to_string()
    } else if quality.eq_ignore_ascii_case("h264") || quality.eq_ignore_ascii_case("mp4-h264") {
        "H264-MP4".to_string()
    } else if let Some((_itag, fb)) = quality.split_once('|') {
        fb.split(':').next().unwrap_or(fb).to_string()
    } else {
        quality.to_string()
    }
}

fn resolve_video_format_spec(quality: &str) -> String {
    let q = quality.trim();
    if q.is_empty() || q.eq_ignore_ascii_case("best") {
        return "best".to_string();
    }
    let lower = q.to_ascii_lowercase();
    if lower == "h264" || lower == "mp4-h264" {
        return "h264".to_string();
    }
    if is_valid_resolution_spec(&lower) {
        return lower;
    }
    if is_valid_itag_spec(&lower) {
        return lower;
    }
    // Composite `itag-<id>|<height>p[60][:mp4|webm]`
    if let Some((primary, fallback)) = lower.split_once('|') {
        let (res, cont) = match fallback.split_once(':') {
            Some((r, c)) => (r, Some(c)),
            None => (fallback, None),
        };
        let cont_ok = cont.is_none_or(|c| c == "mp4" || c == "webm");
        if is_valid_itag_spec(primary) && is_valid_resolution_spec(res) && cont_ok {
            return lower;
        }
    }
    "best".to_string()
}

fn is_valid_resolution_spec(s: &str) -> bool {
    let base = s.strip_suffix("60").unwrap_or(s);
    matches!(
        base,
        "4320p" | "2160p" | "1440p" | "1080p" | "720p" | "480p" | "360p" | "240p" | "144p"
    )
}

fn is_valid_itag_spec(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("itag-") else {
        return false;
    };
    let mut parts = rest.splitn(2, '-');
    let num = parts.next().unwrap_or("");
    let suffix_ok = parts
        .next()
        .is_none_or(|sfx| !sfx.is_empty() && sfx.chars().all(|c| c.is_ascii_alphanumeric()));
    !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) && suffix_ok
}

async fn fetch_youtube_thumbnail_bytes(raw_url: &str, allow_private_net: bool) -> Option<Vec<u8>> {
    let parsed = ruvid::resolver::validate_url_security(
        &ruvid::resolver::ResolveContext::build(
            "RuviTools/2.1",
            4,
            5,
            None,
            &[],
            allow_private_net,
            1,
        )
        .ok()?
        .validate_url(raw_url)
        .ok()?,
        allow_private_net,
    )
    .ok()
    .and_then(|()| {
        ruvid::resolver::ResolveContext::build(
            "RuviTools/2.1",
            4,
            5,
            None,
            &[],
            allow_private_net,
            1,
        )
        .ok()
    })?;
    let u = parsed.validate_url(raw_url).ok()?;
    if !ruvid::resolver::platform::youtube::is_youtube_url(&u) {
        return None;
    }
    let host = u.host_str().unwrap_or("");
    if host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1" {
        return None;
    }
    let vid = ruvid::resolver::platform::youtube::extract_video_id(&u)?;
    let thumb_url = format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg");
    let resp = parsed.client.get(&thumb_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() >= 64 && bytes.len() <= 2 * 1024 * 1024 {
        Some(bytes.to_vec())
    } else {
        None
    }
}

fn validate_download_path(path_str: &str) -> Result<PathBuf, String> {
    if path_str.trim().is_empty() {
        return Err("Dosya yolu boş.".to_string());
    }
    let root_new = download_root();
    let root_old = legacy_download_root();
    let _ = std::fs::create_dir_all(&root_new);
    let canon_new = std::fs::canonicalize(&root_new).unwrap_or(root_new);
    let canon_old = std::fs::canonicalize(&root_old).unwrap_or(root_old);
    let canon_target =
        std::fs::canonicalize(path_str).map_err(|_| "İndirilen dosya bulunamadı.".to_string())?;
    if !canon_target.starts_with(&canon_new) && !canon_target.starts_with(&canon_old) {
        return Err("Yalnızca RuviTube indirme klasöründeki dosyalar açılabilir.".to_string());
    }
    Ok(canon_target)
}

async fn handle_probe_job<W: Write>(
    msg: IncomingMessage,
    out: Arc<Mutex<W>>,
    cfg: HostRuntimeConfig,
) {
    let job_id = msg.job_id.clone();
    if !is_valid_job_id(&job_id) || !is_allowed_request_url(&msg.url, cfg.allow_local_test_url) {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "action": "probe_result",
                "done": true,
                "ok": false,
                "error": "Geçersiz istek veya YouTube adresi."
            }),
        );
        return;
    }

    log_event(&format!("PROBE_START jobId={job_id} url={}", msg.url));
    let ruvid_cfg = RuvidConfig::default();
    let ctx = match ResolveContext::build(
        &ruvid_cfg.network.user_agent,
        ruvid_cfg.network.connect_timeout_seconds,
        20,
        None,
        &[],
        cfg.allow_private_net,
        2,
    ) {
        Ok(c) => c,
        Err(e) => {
            log_event(&format!("PROBE_ERR jobId={job_id} err={e}"));
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "action": "probe_result",
                    "done": true,
                    "ok": false,
                    "error": e.to_string()
                }),
            );
            return;
        }
    };

    match ResolverRegistry::resolve(&ctx, &msg.url).await {
        Ok(source) => {
            let streams: Vec<Value> = source
                .streams
                .iter()
                .map(|s| {
                    let total_bytes = match &s.payload {
                        ruvid::domain::stream::StreamPayload::DirectHttp {
                            total_bytes, ..
                        } => *total_bytes,
                        _ => None,
                    };
                    json!({
                        "id": s.id,
                        "kind": format!("{:?}", s.kind).to_ascii_lowercase(),
                        "container": s.container,
                        "width": s.width,
                        "height": s.height,
                        "fps": s.frame_rate,
                        "codecs": s.codecs,
                        "bandwidth_bps": s.bandwidth_bps,
                        "total_bytes": total_bytes,
                        "language": s.language,
                    })
                })
                .collect();
            let chapters: Vec<Value> = source
                .chapters
                .iter()
                .map(|c| {
                    json!({
                        "title": c.title,
                        "startSec": c.start_seconds,
                        "endSec": c.end_seconds,
                    })
                })
                .collect();
            log_event(&format!(
                "PROBE_OK jobId={job_id} streams={} chapters={}",
                streams.len(),
                chapters.len()
            ));
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "action": "probe_result",
                    "done": true,
                    "ok": true,
                    "title": source.title,
                    "streams": streams,
                    "chapters": chapters
                }),
            );
        }
        Err(e) => {
            log_event(&format!("PROBE_ERR jobId={job_id} err={e}"));
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "action": "probe_result",
                    "done": true,
                    "ok": false,
                    "error": e.to_string()
                }),
            );
        }
    }
}

async fn handle_playlist_job<W: Write>(msg: IncomingMessage, out: Arc<Mutex<W>>) {
    let job_id = msg.job_id.clone();
    let valid_playlist = is_allowed_youtube_url(&msg.url) && has_playlist_id(&msg.url);
    if !is_valid_job_id(&job_id) || !valid_playlist {
        send_native_message(&out, &json!({"jobId":job_id,"action":"playlist_result","done":true,"ok":false,"error":"Geçerli bir YouTube oynatma listesi adresi gerekli."}));
        return;
    }
    let cfg = RuvidConfig::default();
    let ctx = match ResolveContext::build(&cfg.network.user_agent, cfg.network.connect_timeout_seconds, 20, None, &[], false, 2) {
        Ok(ctx) => ctx,
        Err(error) => {
            send_native_message(
                &out,
                &json!({"jobId":job_id,"action":"playlist_result","done":true,"ok":false,"error":error.to_string()}),
            );
            return;
        }
    };
    match fetch_playlist_entries(&ctx, &msg.url).await {
        Ok(entries) => {
            let entries: Vec<Value> = entries
                .into_iter()
                .take(100)
                .map(|entry| {
                    json!({
                        "index": entry.index,
                        "title": entry.title.unwrap_or_default(),
                        "url": entry.url
                    })
                })
                .collect();
            send_native_message(
                &out,
                &json!({"jobId":job_id,"action":"playlist_result","done":true,"ok":true,"entries":entries}),
            );
        }
        Err(error) => send_native_message(
            &out,
            &json!({"jobId":job_id,"action":"playlist_result","done":true,"ok":false,"error":error.to_string()}),
        ),
    }
}

fn has_playlist_id(url: &str) -> bool {
    url.split_once('?')
        .map(|(_, query)| {
            query.split('&').any(|part| {
                part.strip_prefix("list=")
                    .is_some_and(|id| {
                        !id.is_empty()
                            && id
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    })
            })
        })
        .unwrap_or(false)
}

async fn handle_download_job(msg: IncomingMessage, out: Arc<Mutex<io::Stdout>>) {
    handle_download_job_with_config(msg, out, HostRuntimeConfig::default()).await;
}

async fn handle_download_job_with_config<W: Write + Send + 'static>(
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
    log_event(&format!(
        "DOWNLOAD_START jobId={job_id} type={} quality={} jobs={jobs} clip={:?}",
        msg.media_type,
        msg.quality,
        msg.clip.as_ref().map(|c| (c.start_sec, c.end_sec))
    ));

    send_native_message(
        &out,
        &json!({
            "jobId": job_id,
            "percent": 10,
            "jobs": jobs,
            "status": format!("ruvid saf Rust motoru bağlanıyor (Hız x{jobs})...")
        }),
    );

    let is_mp3 = msg.media_type.eq_ignore_ascii_case("MP3");
    let is_m4a = msg.media_type.eq_ignore_ascii_case("M4A");
    let is_aac = msg.media_type.eq_ignore_ascii_case("AAC");
    let is_wav = msg.media_type.eq_ignore_ascii_case("WAV");
    let is_opus = msg.media_type.eq_ignore_ascii_case("OPUS")
        || msg.media_type.eq_ignore_ascii_case("WEBM-AUDIO");
    let is_audio = is_mp3 || is_m4a || is_aac || is_wav || is_opus;

    let tag = quality_tag(&msg.media_type, &msg.quality);
    let final_tag = match msg.clip {
        Some(ref c) => {
            let base = format!(
                "clip {}--{}",
                file_stamp(c.start_sec),
                file_stamp(c.end_sec)
            );
            if let Some(ref chap) = c.chapter_title {
                let clean_chap: String = sanitize_filename(chap).chars().take(40).collect();
                if clean_chap.is_empty() {
                    base
                } else {
                    format!("{clean_chap} [{base}]")
                }
            } else {
                base
            }
        }
        None => tag,
    };

    let job_staging = cfg.staging_dir.join(&job_id);
    let _ = std::fs::create_dir_all(&job_staging);
    let final_dir = cfg.download_dir;
    let _ = std::fs::create_dir_all(&final_dir);

    let format_spec = if is_m4a || is_aac {
        "m4a".to_string()
    } else if is_opus {
        "opus".to_string()
    } else if is_audio {
        "bestaudio".to_string()
    } else {
        resolve_video_format_spec(&msg.quality)
    };

    let raw_template = job_staging.join("%(title)s.%(ext)s");
    let ruvid_cfg = RuvidConfig::default();
    let dl_job = DownloadJob {
        url: msg.url.clone(),
        output_template: Some(raw_template),
        format_spec,
        jobs,
        resume: false,
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
        audio_lang: {
            let trimmed = msg.audio_lang.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        },
        split_chapters: false,
        clip: None,
        start_time: None,
        end_time: None,
        duration_seconds: None,
        allow_private_net: cfg.allow_private_net,
    };

    let out_cb = Arc::clone(&out);
    let job_id_cb = job_id.clone();
    let dl_started = Instant::now();
    let progress_cb: ProgressCallback = Arc::new(move |downloaded, total| {
        let elapsed_s = dl_started.elapsed().as_secs_f64().max(0.05);
        let speed_mb = (downloaded as f64 / (1024.0 * 1024.0)) / elapsed_s;
        let dl_mb = downloaded as f64 / (1024.0 * 1024.0);
        let (pct, status) = match total {
            Some(tot) if tot > 0 => {
                let tot_mb = tot as f64 / (1024.0 * 1024.0);
                let frac = (downloaded as f64 / tot as f64).clamp(0.0, 1.0);
                let p = 12 + (frac * 68.0).round() as u64;
                (
                    p.min(80),
                    format!("İndiriliyor: {dl_mb:.1} / {tot_mb:.1} MB ({speed_mb:.1} MB/sn)"),
                )
            }
            _ => (
                35,
                format!("İndiriliyor: {dl_mb:.1} MB ({speed_mb:.1} MB/sn)"),
            ),
        };
        send_native_message(
            &out_cb,
            &json!({
                "jobId": job_id_cb,
                "percent": pct,
                "jobs": jobs,
                "speed": format!("{speed_mb:.1} MB/s"),
                "status": status
            }),
        );
    });

    let thumb_url_clone = msg.url.clone();
    let allow_priv = cfg.allow_private_net;
    let thumb_task =
        tokio::spawn(
            async move { fetch_youtube_thumbnail_bytes(&thumb_url_clone, allow_priv).await },
        );

    let outcome =
        match execute_download_with_progress(&ruvid_cfg, dl_job, false, Some(progress_cb)).await {
            Ok(o) => o,
            Err(e) => {
                thumb_task.abort();
                log_event(&format!("DOWNLOAD_FAILED jobId={job_id} err={e}"));
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
    let cover_bytes = thumb_task.await.ok().flatten();

    log_event(&format!(
        "DOWNLOAD_RES jobId={job_id} stream={} bytes={} path={}",
        outcome.stream_id, outcome.bytes_downloaded, outcome.output_path
    ));

    let mut working_path = PathBuf::from(&outcome.output_path);
    let mut clean_title = sanitize_filename(&outcome.title);
    if clean_title.chars().count() > 100 {
        clean_title = clean_title.chars().take(100).collect();
    }

    let backend = NativeMediaBackend::new();
    let ruvic_cfg = RuvicConfig::default();

    let clip_start_str = msg.clip.as_ref().map(|c| format!("{:.3}", c.start_sec));
    let clip_dur = msg
        .clip
        .as_ref()
        .map(|c| (c.end_sec - c.start_sec).max(0.1));

    // For video/M4A/AAC clips, trim container directly first; for MP3/WAV, decode+slice in one pass!
    if !is_mp3
        && !is_wav
        && let Some(ref c) = msg.clip
    {
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 82,
                "status": "ruvic saf Rust medya motoru ile kesit kırpılıyor..."
            }),
        );
        let src_ext = working_path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .unwrap_or("mp4");
        let trimmed_path = job_staging.join(format!("{clean_title} [{final_tag}].{src_ext}"));
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
            log_event(&format!("TRIM_FAILED jobId={job_id} err={e}"));
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

    let final_ext = if is_mp3 {
        "mp3"
    } else if is_wav {
        "wav"
    } else if is_aac {
        "aac"
    } else if is_m4a {
        working_path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.eq_ignore_ascii_case("m4a") || e.eq_ignore_ascii_case("webm"))
            .unwrap_or("m4a")
    } else {
        working_path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .unwrap_or("mp4")
    };
    let final_file_name = format!("{clean_title} [{final_tag}].{final_ext}");
    let final_dest = final_dir.join(&final_file_name);

    if is_mp3 || is_wav || is_aac {
        let label = if is_mp3 {
            "MP3"
        } else if is_wav {
            "WAV"
        } else {
            "AAC"
        };
        send_native_message(
            &out,
            &json!({
                "jobId": job_id,
                "percent": 88,
                "status": format!("ruvic saf Rust motoru ile {label}'e dönüştürülüyor...")
            }),
        );
        let bitrate = if is_mp3 {
            Some(normalize_mp3_bitrate_arg(&msg.quality))
        } else {
            None
        };
        log_event(&format!(
            "CONVERT_START jobId={job_id} to={final_ext} bitrate={bitrate:?}"
        ));
        let audio_req = ExtractAudioRequest {
            input: working_path.clone(),
            output: final_dest.clone(),
            to: Some(final_ext.to_string()),
            copy: is_aac,
            overwrite: true,
            audio_stream: None,
            bitrate,
            start: if is_mp3 || is_wav {
                clip_start_str
            } else {
                None
            },
            duration: if is_mp3 || is_wav { clip_dur } else { None },
        };
        let source_duration = backend
            .probe(&working_path)
            .await
            .ok()
            .and_then(|info| info.container.duration_seconds);
        let conversion_duration = clip_dur.or(source_duration);
        let progress = NativeProgressSink {
            out: Arc::clone(&out),
            job_id: job_id.clone(),
            total_duration_us: conversion_duration
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .map(|duration| (duration * 1_000_000.0) as u64),
            label,
        };
        if let Err(e) =
            execute_extract_audio_with_sink(&backend, &ruvic_cfg, audio_req, &progress).await
        {
            log_event(&format!("CONVERT_FAILED jobId={job_id} err={e}"));
            send_native_message(
                &out,
                &json!({
                    "jobId": job_id,
                    "percent": 0,
                    "done": true,
                    "ok": false,
                    "status": format!("{label} dönüşüm hatası"),
                    "error": e.to_string()
                }),
            );
            let _ = std::fs::remove_dir_all(&job_staging);
            return;
        }
        let _ = std::fs::remove_file(&working_path);
    } else if is_m4a
        && msg.clip.is_some()
        && working_path.extension().and_then(|e| e.to_str()) == Some("m4a")
    {
        let remux_req = RemuxRequest {
            input: working_path.clone(),
            secondary_input: None,
            output: final_dest.clone(),
            overwrite: true,
            video_stream: None,
            audio_stream: None,
        };
        if execute_remux(&backend, &ruvic_cfg, remux_req, false)
            .await
            .is_err()
        {
            if final_dest.exists() {
                let _ = std::fs::remove_file(&final_dest);
            }
            if std::fs::rename(&working_path, &final_dest).is_err() {
                let _ = std::fs::copy(&working_path, &final_dest);
            }
        }
        let _ = std::fs::remove_file(&working_path);
    } else {
        if final_dest.exists() {
            let _ = std::fs::remove_file(&final_dest);
        }
        if std::fs::rename(&working_path, &final_dest).is_err() {
            if let Err(e) = std::fs::copy(&working_path, &final_dest) {
                log_event(&format!("MOVE_FAILED jobId={job_id} err={e}"));
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

    // Embed rich album/artist/title/year/cover-art metadata into MP3 and fMP4 (.mp4 / .m4a) outputs.
    let meta_source_title = if !outcome.title.trim().is_empty() && outcome.title != "youtube_video"
    {
        outcome.title.as_str()
    } else if !msg.title.trim().is_empty() {
        msg.title.as_str()
    } else {
        clean_title.as_str()
    };
    let channel_hint = (!msg.channel.trim().is_empty()).then_some(msg.channel.trim());
    let mut rich_meta =
        parse_smart_title_metadata(meta_source_title, channel_hint, Some(msg.url.as_str()));
    if let Some(ref c) = msg.clip
        && let Some(ref chap) = c.chapter_title
        && !chap.trim().is_empty()
    {
        rich_meta.title = Some(chap.trim().to_string());
    }
    rich_meta.cover_jpeg = cover_bytes;

    if is_mp3 {
        let _ = write_id3v23_to_mp3_file(&final_dest, &rich_meta);
    } else if final_ext.eq_ignore_ascii_case("mp4") || final_ext.eq_ignore_ascii_case("m4a") {
        let _ = inject_fmp4_itunes_metadata_file(
            &final_dest,
            rich_meta.title.as_deref().unwrap_or(&clean_title),
            rich_meta.artist.as_deref().unwrap_or("YouTube"),
            rich_meta.album.as_deref().unwrap_or(&clean_title),
            rich_meta.year.as_deref().unwrap_or("2026"),
            rich_meta.comment.as_deref().unwrap_or(&msg.url),
            rich_meta.cover_jpeg.as_deref(),
        );
    }

    let _ = std::fs::remove_dir_all(&job_staging);
    let file_size = std::fs::metadata(&final_dest).map(|m| m.len()).unwrap_or(0);
    log_event(&format!(
        "DOWNLOAD_OK jobId={job_id} size={file_size} file={}",
        final_dest.display()
    ));

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
                version: "2.2.0",
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
        "install_extension_update" => {
            let result = run_extension_update(&msg.version, &msg.url);
            match result {
                Ok(()) => send_native_message(
                    out,
                    &json!({
                        "action": "extension_update_result",
                        "ok": true,
                        "version": msg.version
                    }),
                ),
                Err(error) => send_native_message(
                    out,
                    &json!({
                        "action": "extension_update_result",
                        "ok": false,
                        "version": msg.version,
                        "error": error
                    }),
                ),
            }
            true
        }
        "download" | "probe" | "playlist" => false,
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

fn run_extension_update(version: &str, url: &str) -> Result<(), String> {
    if !is_allowed_extension_update_request(version, url) {
        return Err("Güncelleme sürümü veya adresi geçersiz.".to_string());
    }

    let app_dir = std::env::current_exe()
        .map_err(|e| format!("NativeHost konumu bulunamadı: {e}"))?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "NativeHost kurulum klasörü bulunamadı.".to_string())?;
    let script = app_dir.join("Update-Extension.ps1");
    if !script.is_file() {
        return Err("Otomatik güncelleme bileşeni kurulu değil; All-in-One kurulumunu bir kez çalıştırın.".to_string());
    }

    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg("-Version")
        .arg(version)
        .arg("-Url")
        .arg(url)
        .output()
        .map_err(|e| format!("Güncelleme aracı başlatılamadı: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            format!("Güncelleme aracı hata kodu {} ile kapandı.", output.status)
        } else {
            detail.chars().take(500).collect()
        })
    }
}

fn is_allowed_extension_update_request(version: &str, url: &str) -> bool {
    let version_parts: Vec<&str> = version.split('.').collect();
    if version_parts.len() != 3
        || version_parts.iter().any(|part| {
            part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return false;
    }

    let source_url = "https://github.com/mtahiryilmazz/RuviTools/archive/refs/heads/main.zip";
    let release_url = format!(
        "https://github.com/mtahiryilmazz/RuviTools/releases/download/v{version}/RuviTube-v{version}-chrome.zip"
    );
    url == source_url || url == release_url
}

fn process_native_stream_sync<R: Read, W: Write>(
    mut reader: R,
    out: Arc<Mutex<W>>,
    mut on_async: impl FnMut(IncomingMessage, Arc<Mutex<W>>),
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
            on_async(msg, out.clone());
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("RuviTube NativeHost 2.2.0 (ruvid+ruvic pure-Rust in-process)");
        return;
    }

    log_event(&format!("HOST_START v2.2.0 pid={}", std::process::id()));
    let stdout_lock = Arc::new(Mutex::new(io::stdout()));
    let stdin = io::stdin();
    let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    process_native_stream_sync(stdin, stdout_lock, |msg, out_clone| {
        let h = tokio::spawn(async move {
            if msg.action == "playlist" {
                handle_playlist_job(msg, out_clone).await;
            } else if msg.action == "probe" {
                handle_probe_job(msg, out_clone, HostRuntimeConfig::default()).await;
            } else {
                handle_download_job(msg, out_clone).await;
            }
        });
        handles.push(h);
    });
    for h in handles {
        let _ = h.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_update_request_is_limited_to_ruvitools_archives() {
        assert!(is_allowed_extension_update_request(
            "2.7.0",
            "https://github.com/mtahiryilmazz/RuviTools/archive/refs/heads/main.zip"
        ));
        assert!(is_allowed_extension_update_request(
            "2.7.0",
            "https://github.com/mtahiryilmazz/RuviTools/releases/download/v2.7.0/RuviTube-v2.7.0-chrome.zip"
        ));
        assert!(!is_allowed_extension_update_request(
            "2.7.0",
            "https://example.com/RuviTube-v2.7.0-chrome.zip"
        ));
        assert!(!is_allowed_extension_update_request(
            "2.7.0",
            "https://github.com/mtahiryilmazz/RuviTools/releases/download/v2.6.0/RuviTube-v2.6.0-chrome.zip"
        ));
        assert!(!is_allowed_extension_update_request(
            "2.7",
            "https://github.com/mtahiryilmazz/RuviTools/archive/refs/heads/main.zip"
        ));
    }

    #[test]
    fn test_validate_clip_normal() {
        let clip = ClipSpec {
            start_sec: 10.0,
            end_sec: 30.0,
            chapter_title: None,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_long_duration_over_6h() {
        let clip = ClipSpec {
            start_sec: 0.0,
            end_sec: 36738.125,
            chapter_title: None,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_over_24h() {
        let clip = ClipSpec {
            start_sec: 86500.0,
            end_sec: 86600.0,
            chapter_title: None,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_ultra_long_100h() {
        let clip = ClipSpec {
            start_sec: 0.0,
            end_sec: 360000.0,
            chapter_title: None,
        };
        assert!(validate_clip(&clip).is_ok());
    }

    #[test]
    fn test_validate_clip_invalid_negative_start() {
        let clip = ClipSpec {
            start_sec: -1.0,
            end_sec: 10.0,
            chapter_title: None,
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
            chapter_title: None,
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
            chapter_title: None,
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
            chapter_title: None,
        };
        assert_eq!(validate_clip(&clip), Err("Kesit zamanı geçersiz."));
    }

    #[test]
    fn test_validate_clip_invalid_infinite() {
        let clip = ClipSpec {
            start_sec: 0.0,
            end_sec: f64::INFINITY,
            chapter_title: None,
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
    fn test_is_allowed_youtube_url_accepts_all_yt_forms_and_rejects_invalid() {
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/watch?v=jNQXAC9IVRw"
        ));
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/watch?v=jNQXAC9IVRw&t=12s#frag"
        ));
        assert!(is_allowed_youtube_url(
            "https://m.youtube.com/watch?v=jNQXAC9IVRw"
        ));
        assert!(is_allowed_youtube_url(
            "https://music.youtube.com/watch?v=jNQXAC9IVRw"
        ));
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/playlist?list=PL1234567890"
        ));
        assert!(is_allowed_youtube_url(
            "https://music.youtube.com/playlist?list=PL1234567890"
        ));
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/shorts/jNQXAC9IVRw"
        ));
        assert!(is_allowed_youtube_url(
            "https://www.youtube.com/live/jNQXAC9IVRw"
        ));
        assert!(is_allowed_youtube_url("https://youtu.be/jNQXAC9IVRw"));

        assert!(!is_allowed_youtube_url(""));
        assert!(!is_allowed_youtube_url(
            "http://www.youtube.com/watch?v=jNQXAC9IVRw"
        ));
        assert!(!is_allowed_youtube_url("https://evil.example/watch?v=123"));
        assert!(!is_allowed_youtube_url("https://www.youtube.com/playlist?list=bad value"));
        assert!(!is_allowed_youtube_url(
            "https://www.youtube.com/watch?v=abc def"
        ));
        let over_limit = format!("https://www.youtube.com/watch?v={}", "a".repeat(2040));
        assert!(!is_allowed_youtube_url(&over_limit));
    }

    #[test]
    fn test_playlist_url_requires_a_nonempty_playlist_id() {
        assert!(has_playlist_id("https://www.youtube.com/playlist?list=PL123_abc-"));
        assert!(has_playlist_id("https://www.youtube.com/watch?v=jNQXAC9IVRw&list=PL123"));
        assert!(!has_playlist_id("https://www.youtube.com/playlist?list="));
        assert!(!has_playlist_id("https://www.youtube.com/playlist?notlist=PL123"));
        assert!(!has_playlist_id("https://www.youtube.com/playlist?list=bad%20id"));
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
    fn test_quality_tag_and_bitrate_mapping() {
        assert_eq!(quality_tag("MP3", "320kbps"), "320kbps");
        assert_eq!(quality_tag("MP3", "256kbps"), "256kbps");
        assert_eq!(quality_tag("MP3", "192kbps"), "192kbps");
        assert_eq!(quality_tag("MP3", "128kbps"), "128kbps");
        assert_eq!(quality_tag("MP3", "vbr"), "VBR");
        assert_eq!(quality_tag("M4A", "original"), "M4A");
        assert_eq!(quality_tag("WAV", "original"), "WAV");
        assert_eq!(quality_tag("Video", ""), "En iyi");
        assert_eq!(quality_tag("Video", "best"), "En iyi");
        assert_eq!(quality_tag("Video", "2160p"), "2160p");

        assert_eq!(normalize_mp3_bitrate_arg("320kbps"), "320k");
        assert_eq!(normalize_mp3_bitrate_arg("vbr"), "vbr");
        assert_eq!(normalize_mp3_bitrate_arg("128kbps"), "128k");
        assert_eq!(resolve_video_format_spec("2160p"), "2160p");
        assert_eq!(resolve_video_format_spec("itag-137"), "itag-137");
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
        assert_eq!(responses[0]["version"], json!("2.2.0"));
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
            channel: String::new(),
            title: String::new(),
            version: String::new(),
            audio_lang: String::new(),
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
                .contains("Hız x8"),
            "expected status to contain Hız x8, got: {}",
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

        assert!(
            range_requests.load(Ordering::Relaxed) >= 8,
            "expected multiple HTTP Range requests for --jobs 8, got {}",
            range_requests.load(Ordering::Relaxed)
        );
    }
}

#[cfg(test)]
mod format_spec_tests {
    use super::{quality_tag, resolve_video_format_spec};

    #[test]
    fn resolves_high_fps_composite_and_opus() {
        assert_eq!(resolve_video_format_spec("2160p60"), "2160p60");
        assert_eq!(resolve_video_format_spec("1440P"), "1440p");
        assert_eq!(resolve_video_format_spec("itag-313"), "itag-313");
        assert_eq!(resolve_video_format_spec("itag-140-9"), "itag-140-9");
        assert_eq!(
            resolve_video_format_spec("itag-313|2160p:webm"),
            "itag-313|2160p:webm"
        );
        assert_eq!(
            resolve_video_format_spec("itag-299|1080p60:mp4"),
            "itag-299|1080p60:mp4"
        );
        assert_eq!(resolve_video_format_spec("itag-1|999p:avi"), "best");
        assert_eq!(resolve_video_format_spec("../evil"), "best");
        assert_eq!(quality_tag("OPUS", "original"), "Opus");
    }
}
