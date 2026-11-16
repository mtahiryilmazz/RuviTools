use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use eframe::egui::{self, Color32, RichText, ScrollArea, Stroke, Vec2};
use ruvic::backend::native::NativeMediaBackend;
use ruvic::backend::native::container_probe::probe_media_file;
use ruvic::config::RuvicConfig;
use ruvic::domain::conversion_job::OperationResult;
use ruvic::domain::media_info::MediaInfo;
use ruvic::operations::extract_audio::{ExtractAudioRequest, execute_extract_audio};
use ruvic::operations::remux::{RemuxRequest, execute_remux};
use ruvic::operations::thumbnail::{ThumbnailRequest, execute_thumbnail};
use ruvic::operations::trim::{TrimRequest, execute_trim};
use ruvid::config::RuvidConfig;
use ruvid::domain::download_job::{DownloadJob, DownloadOutcome};
use ruvid::domain::media_manifest::redact_media_source;
use ruvid::domain::media_source::MediaSource;
use ruvid::downloader::execute_download;
use ruvid::resolver::ResolveContext;
use ruvid::resolver::registry::{PlaylistEntry, ResolverRegistry, fetch_playlist_entries};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Downloader,
    MediaStudio,
    PlaylistQueue,
    Telemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOperationKind {
    RemuxFaststart,
    TrimRange,
    ExtractAudioAac,
    ExtractAudioM4a,
    Thumbnail,
}

impl MediaOperationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RemuxFaststart => "Faststart Remux (MP4 / WebM)",
            Self::TrimRange => "Zaman Araligi Kesme (Trim)",
            Self::ExtractAudioAac => "Ses Cikarimi (ADTS .aac)",
            Self::ExtractAudioM4a => "Ses Cikarimi (Kapsayici .m4a)",
            Self::Thumbnail => "Kucuk Resim Yakala (Thumbnail)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StudioTheme {
    Dark,
    Light,
}

impl Default for StudioTheme {
    fn default() -> Self {
        Self::Dark
    }
}

impl StudioTheme {
    pub fn visuals(&self) -> egui::Visuals {
        match self {
            Self::Dark => {
                let mut visuals = egui::Visuals::dark();
                visuals.panel_fill = Color32::from_rgb(13, 18, 30);
                visuals.window_fill = Color32::from_rgb(18, 25, 40);
                visuals.extreme_bg_color = Color32::from_rgb(9, 13, 22);
                visuals
            }
            Self::Light => {
                let mut visuals = egui::Visuals::light();
                visuals.panel_fill = Color32::from_rgb(248, 250, 252);
                visuals.window_fill = Color32::from_rgb(255, 255, 255);
                visuals.extreme_bg_color = Color32::from_rgb(241, 245, 249);
                visuals
            }
        }
    }

    pub fn card_frame(&self) -> egui::Frame {
        match self {
            Self::Dark => egui::Frame::NONE
                .fill(Color32::from_rgb(22, 30, 49))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 60, 90)))
                .inner_margin(12.0)
                .corner_radius(8.0),
            Self::Light => egui::Frame::NONE
                .fill(Color32::from_rgb(255, 255, 255))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(226, 232, 240)))
                .inner_margin(12.0)
                .corner_radius(8.0),
        }
    }

    pub fn accent_primary(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(52, 211, 153),
            Self::Light => Color32::from_rgb(16, 149, 103),
        }
    }

    pub fn accent_secondary(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(56, 189, 248),
            Self::Light => Color32::from_rgb(2, 132, 199),
        }
    }

    pub fn accent_purple(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(167, 139, 250),
            Self::Light => Color32::from_rgb(124, 58, 237),
        }
    }

    pub fn text_muted(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(156, 163, 175),
            Self::Light => Color32::from_rgb(100, 116, 139),
        }
    }

    pub fn status_text(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(209, 213, 219),
            Self::Light => Color32::from_rgb(51, 65, 85),
        }
    }

    pub fn error_banner_fill(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(69, 18, 28),
            Self::Light => Color32::from_rgb(254, 242, 242),
        }
    }

    pub fn error_banner_stroke(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(244, 63, 94),
            Self::Light => Color32::from_rgb(248, 113, 113),
        }
    }

    pub fn error_banner_text(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(253, 164, 175),
            Self::Light => Color32::from_rgb(159, 18, 57),
        }
    }

    pub fn card_header_text(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(243, 244, 246),
            Self::Light => Color32::from_rgb(15, 23, 42),
        }
    }

    pub fn render_card<R>(
        &self,
        ui: &mut egui::Ui,
        title: Option<&str>,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.card_frame()
            .show(ui, |ui| {
                if let Some(t) = title {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(t)
                                .size(15.0)
                                .strong()
                                .color(self.accent_secondary()),
                        );
                    });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(6.0);
                }
                add_contents(ui)
            })
            .inner
    }
}

#[derive(Debug, Clone)]
pub struct TelemetryEntry {
    pub task_name: String,
    pub elapsed_ms: f64,
    pub bytes_processed: u64,
    pub chunks: usize,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct StudioTelemetry {
    pub history: Vec<TelemetryEntry>,
    pub total_bytes_downloaded: u64,
    pub total_bytes_processed: u64,
    pub jobs_completed: usize,
}

pub enum GuiEvent {
    SourceResolved {
        source: Box<MediaSource>,
        elapsed_ms: f64,
    },
    DownloadFinished {
        outcome: DownloadOutcome,
        elapsed_ms: f64,
    },
    MediaProbed {
        info: Box<MediaInfo>,
        elapsed_ms: f64,
    },
    MediaOperationFinished {
        result: Box<OperationResult>,
        elapsed_ms: f64,
    },
    PlaylistResolved {
        entries: Vec<PlaylistEntry>,
        elapsed_ms: f64,
    },
    PlaylistBatchFinished {
        succeeded: usize,
        failed: usize,
        bytes_total: u64,
        elapsed_ms: f64,
    },
    TaskFailed {
        context: String,
        error: String,
    },
}

pub struct RuviStudioApp {
    pub active_tab: ActiveTab,
    pub runtime: Arc<tokio::runtime::Runtime>,
    tx: Sender<GuiEvent>,
    rx: Receiver<GuiEvent>,
    pub busy: bool,
    pub status_line: String,
    pub error_banner: Option<String>,

    // Downloader tab state
    pub url_input: String,
    pub output_path_input: String,
    pub format_spec: String,
    pub jobs_count: usize,
    pub resume_enabled: bool,
    pub overwrite_enabled: bool,
    pub download_subtitles: bool,
    pub auto_subtitles: bool,
    pub subtitle_lang: String,
    pub subtitle_format: String,
    pub audio_lang: String,
    pub split_chapters: bool,
    pub clip_start: String,
    pub clip_end: String,
    pub clip_duration: String,
    pub write_manifest_path: String,
    pub resolved_source: Option<MediaSource>,
    pub last_download: Option<DownloadOutcome>,

    // Media Studio (ruvic) tab state
    pub media_input_path: String,
    pub media_secondary_audio_path: String,
    pub media_output_path: String,
    pub media_op: MediaOperationKind,
    pub trim_start: String,
    pub trim_duration: f64,
    pub thumbnail_at: String,
    pub probed_info: Option<MediaInfo>,
    pub last_media_op: Option<OperationResult>,

    // Playlist tab state
    pub playlist_url: String,
    pub playlist_output_dir: String,
    pub playlist_concurrency: usize,
    pub playlist_skip_errors: bool,
    pub playlist_entries: Vec<(bool, PlaylistEntry)>,

    // Telemetry & Modern UI State
    pub telemetry: StudioTelemetry,
    pub current_theme: StudioTheme,
    pub getube_dir: String,
    pub getube_status: String,
    pub getube_native_host_found: bool,
    pub getube_extension_found: bool,
    theme_initialized: bool,
}

impl Default for RuviStudioApp {
    fn default() -> Self {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .expect("tokio runtime"),
        );
        Self::new(runtime)
    }
}

impl RuviStudioApp {
    pub fn new(runtime: Arc<tokio::runtime::Runtime>) -> Self {
        let (tx, rx) = channel();
        let default_getube_dir = "d:\\mtahiryilmazz\\Github\\GeTube\\GeTube_v2.0.0".to_string();
        let mut app = Self {
            active_tab: ActiveTab::Downloader,
            runtime,
            tx,
            rx,
            busy: false,
            status_line:
                "Hazir — Saf Rust ruvid + ruvic motorlari bagli (Sifir harici bagimlilik)."
                    .to_string(),
            error_banner: None,
            current_theme: StudioTheme::Dark,
            getube_dir: default_getube_dir,
            getube_status: "Kuruluma hazır. 'GeTube Eklentisini Kur' butonuna tıklayarak Native Host'u kaydedebilirsiniz.".to_string(),
            getube_native_host_found: false,
            getube_extension_found: false,
            url_input: "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_string(),
            output_path_input: "./downloads/%(title)s.%(ext)s".to_string(),
            format_spec: "best".to_string(),
            jobs_count: 4,
            resume_enabled: true,
            overwrite_enabled: true,
            download_subtitles: true,
            auto_subtitles: false,
            subtitle_lang: String::new(),
            subtitle_format: "srt".to_string(),
            audio_lang: String::new(),
            split_chapters: false,
            clip_start: String::new(),
            clip_end: String::new(),
            clip_duration: String::new(),
            write_manifest_path: String::new(),
            resolved_source: None,
            last_download: None,
            media_input_path: String::new(),
            media_secondary_audio_path: String::new(),
            media_output_path: "./downloads/output_faststart.mp4".to_string(),
            media_op: MediaOperationKind::RemuxFaststart,
            trim_start: "00:00:02".to_string(),
            trim_duration: 5.0,
            thumbnail_at: "00:00:02".to_string(),
            probed_info: None,
            last_media_op: None,
            playlist_url: String::new(),
            playlist_output_dir: "./downloads/playlist".to_string(),
            playlist_concurrency: 2,
            playlist_skip_errors: true,
            playlist_entries: Vec::new(),
            telemetry: StudioTelemetry::default(),
            theme_initialized: false,
        };
        app.check_getube_status();
        app
    }

    pub fn theme(&self) -> StudioTheme {
        self.current_theme
    }

    pub fn set_theme(&mut self, theme: StudioTheme) {
        self.current_theme = theme;
    }

    pub fn render_card<R>(
        &self,
        ui: &mut egui::Ui,
        title: Option<&str>,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.current_theme.render_card(ui, title, add_contents)
    }

    pub fn check_getube_status(&mut self) {
        let mut base = PathBuf::from(self.getube_dir.trim());
        if !base.exists() {
            let alt_path = PathBuf::from("../GeTube/GeTube_v2.0.0");
            if alt_path.exists() {
                base = alt_path;
                self.getube_dir = base.to_string_lossy().to_string();
            }
        }

        let host_bin = base.join("bin").join("NativeHost.exe");
        let ext_manifest = base.join("extension").join("manifest.json");

        self.getube_native_host_found = host_bin.is_file();
        self.getube_extension_found = ext_manifest.is_file();

        if self.getube_native_host_found && self.getube_extension_found {
            self.getube_status = "✅ GeTube eklentisi ve Native Host hazır durumda.".to_string();
        } else if self.getube_extension_found {
            self.getube_status = "ℹ GeTube Chrome eklentisi bulundu. Native Host kurulumu için aşağıdaki butona tıklayın.".to_string();
        } else {
            self.getube_status =
                format!("⚠ Belirtilen dizinde eklenti bulunamadı: {}", base.display());
        }
    }

    pub fn install_getube(&mut self) {
        let base = PathBuf::from(self.getube_dir.trim());
        let installer_ps1 = base.join("installer").join("Install.ps1");
        let installer_cmd = base.join("installer").join("Kurulum.cmd");
        let ext_dir = base.join("extension");

        if !base.exists() {
            self.getube_status = format!("❌ GeTube dizini bulunamadı: {}", base.display());
            return;
        }

        #[cfg(target_os = "windows")]
        {
            if installer_ps1.is_file() {
                let _ = std::process::Command::new("powershell.exe")
                    .args([
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                        installer_ps1.to_str().unwrap_or_default(),
                    ])
                    .spawn();
                self.getube_status =
                    "🚀 GeTube kurulum betiği çalıştırıldı. Native Host kaydediliyor...".to_string();
            } else if installer_cmd.is_file() {
                let _ = std::process::Command::new("cmd.exe")
                    .args(["/C", installer_cmd.to_str().unwrap_or_default()])
                    .spawn();
                self.getube_status = "🚀 GeTube Kurulum.cmd çalıştırıldı.".to_string();
            } else {
                self.getube_status =
                    format!("⚠ Kurulum betiği bulunamadı: {}", installer_ps1.display());
            }

            if ext_dir.is_dir() {
                let _ = std::process::Command::new("explorer.exe")
                    .arg(&ext_dir)
                    .spawn();
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            self.getube_status = format!("ℹ GeTube eklenti dizini: {}", ext_dir.display());
        }

        self.check_getube_status();
        self.status_line = "GeTube kurulumu tetiklendi. Chrome'da 'chrome://extensions' sayfasından 'Paketlenmemiş öge yükle' ile eklentiyi aktifleştirebilirsiniz.".to_string();
    }

    pub fn open_getube_extension_dir(&self) {
        let ext_dir = PathBuf::from(self.getube_dir.trim()).join("extension");
        if ext_dir.is_dir() {
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("explorer.exe")
                .arg(&ext_dir)
                .spawn();
        }
    }

    pub fn open_chrome_extensions_page(&self) {
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd.exe")
            .args(["/C", "start", "chrome", "chrome://extensions"])
            .spawn();
    }

    pub fn check_ruvitube_status(&mut self) {
        self.check_getube_status();
    }

    pub fn install_ruvitube(&mut self) {
        self.install_getube();
    }

    pub fn open_ruvitube_extension_dir(&self) {
        self.open_getube_extension_dir();
    }

    pub fn render_getube_setup_card(&mut self, ui: &mut egui::Ui) {
        self.render_ruvitube_setup_card(ui);
    }

    pub fn render_ruvitube_setup_card(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        theme.render_card(ui, Some("🧩 RuviTube Chrome Eklentisi Entegrasyonu"), |ui| {
            ui.label(
                RichText::new(
                    "RuviTube (GeTube), YouTube web oynatıcısına doğrudan klip kesme ve indirme düğmesi ekleyen Chrome eklentisidir. RuviTools native motoru ile entegre çalışır.",
                )
                .color(theme.text_muted()),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Eklenti / Kurulum Dizini:");
                let edit_res = ui.add(
                    egui::TextEdit::singleline(&mut self.getube_dir)
                        .desired_width(360.0)
                        .hint_text("d:\\mtahiryilmazz\\Github\\GeTube\\GeTube_v2.0.0"),
                );
                if edit_res.changed() {
                    self.check_getube_status();
                }
                if ui.button("🔍 Yeniden Tara").clicked() {
                    self.check_getube_status();
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let host_text = if self.getube_native_host_found {
                    RichText::new("🟢 Native Host: Bulundu (bin/NativeHost.exe)")
                        .color(theme.accent_primary())
                } else {
                    RichText::new("🔴 Native Host: Bulunamadı (bin/NativeHost.exe)")
                        .color(theme.error_banner_stroke())
                };
                ui.label(host_text);

                ui.separator();

                let ext_text = if self.getube_extension_found {
                    RichText::new("🟢 Chrome Eklentisi: Bulundu (manifest.json)")
                        .color(theme.accent_primary())
                } else {
                    RichText::new("🔴 Chrome Eklentisi: Bulunamadı")
                        .color(theme.error_banner_stroke())
                };
                ui.label(ext_text);
            });

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("🚀 RuviTube Eklentisini Kur / Yapılandır")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(theme.accent_primary())
                        .min_size(Vec2::new(260.0, 30.0)),
                    )
                    .clicked()
                {
                    self.install_getube();
                }

                if ui.button("📂 Eklenti Klasörünü Aç (extension)").clicked() {
                    self.open_getube_extension_dir();
                }

                if ui
                    .button("🌐 Chrome Eklentilerini Aç (chrome://extensions)")
                    .clicked()
                {
                    self.open_chrome_extensions_page();
                }
            });

            ui.add_space(4.0);
            ui.label(
                RichText::new(&self.getube_status)
                    .size(12.5)
                    .color(theme.status_text()),
            );
        });
    }

    pub fn build_download_job(&self) -> DownloadJob {
        let duration_seconds = self
            .clip_duration
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|&d| d > 0.0);
        DownloadJob {
            url: self.url_input.trim().to_string(),
            output_template: if self.output_path_input.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(self.output_path_input.trim()))
            },
            format_spec: if self.format_spec.trim().is_empty() {
                "best".to_string()
            } else {
                self.format_spec.trim().to_string()
            },
            jobs: self.jobs_count.max(1),
            resume: self.resume_enabled,
            overwrite: self.overwrite_enabled,
            limit_rate: None,
            retry_count: 3,
            timeout_seconds: 30,
            proxy: None,
            headers: Vec::new(),
            write_manifest: if self.write_manifest_path.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(self.write_manifest_path.trim()))
            },
            to_stdout: false,
            subtitles: self.download_subtitles || self.auto_subtitles,
            subtitles_lang: if self.subtitle_lang.trim().is_empty() {
                None
            } else {
                Some(self.subtitle_lang.trim().to_string())
            },
            sub_format: self.subtitle_format.clone(),
            auto_subs: self.auto_subtitles,
            audio_lang: if self.audio_lang.trim().is_empty() {
                None
            } else {
                Some(self.audio_lang.trim().to_string())
            },
            split_chapters: self.split_chapters,
            clip: None,
            start_time: if self.clip_start.trim().is_empty() {
                None
            } else {
                Some(self.clip_start.trim().to_string())
            },
            end_time: if self.clip_end.trim().is_empty() {
                None
            } else {
                Some(self.clip_end.trim().to_string())
            },
            duration_seconds,
            allow_private_net: false,
        }
    }

    pub fn poll_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                GuiEvent::SourceResolved { source, elapsed_ms } => {
                    self.busy = false;
                    self.error_banner = None;
                    self.status_line = format!(
                        "Kaynak cozuldu: '{}' ({} akis, {} altyazi, {} bolum) — {:.1} ms",
                        source.title,
                        source.streams.len(),
                        source.subtitles.len(),
                        source.chapters.len(),
                        elapsed_ms
                    );
                    self.telemetry.history.push(TelemetryEntry {
                        task_name: format!("ruvid info ({})", source.title),
                        elapsed_ms,
                        bytes_processed: 0,
                        chunks: source.streams.len(),
                        detail: format!(
                            "{} akis, {} altyazi, {} bolum",
                            source.streams.len(),
                            source.subtitles.len(),
                            source.chapters.len()
                        ),
                    });
                    self.resolved_source = Some(*source);
                }
                GuiEvent::DownloadFinished {
                    outcome,
                    elapsed_ms,
                } => {
                    self.busy = false;
                    self.error_banner = None;
                    let mb_s = if elapsed_ms > 0.0 {
                        (outcome.bytes_downloaded as f64 / (1024.0 * 1024.0))
                            / (elapsed_ms / 1000.0)
                    } else {
                        0.0
                    };
                    self.status_line = format!(
                        "Indirme tamamlandi: {} ({} B, {} parca, {:.2} MB/sn) -> {}",
                        outcome.title,
                        outcome.bytes_downloaded,
                        outcome.chunks_total,
                        mb_s,
                        outcome.output_path
                    );
                    self.media_input_path = outcome.output_path.clone();
                    self.telemetry.total_bytes_downloaded += outcome.bytes_downloaded;
                    self.telemetry.jobs_completed += 1;
                    self.telemetry.history.push(TelemetryEntry {
                        task_name: format!("ruvid get [{}]", outcome.stream_id),
                        elapsed_ms,
                        bytes_processed: outcome.bytes_downloaded,
                        chunks: outcome.chunks_total,
                        detail: outcome.output_path.clone(),
                    });
                    self.last_download = Some(outcome);
                }
                GuiEvent::MediaProbed { info, elapsed_ms } => {
                    self.busy = false;
                    self.error_banner = None;
                    self.status_line = format!(
                        "Konteyner yoklandi: {} ({} akis, {:.2} sn) — {:.2} ms",
                        info.container.format_name,
                        info.streams.len(),
                        info.container.duration_seconds.unwrap_or(0.0),
                        elapsed_ms
                    );
                    self.telemetry.history.push(TelemetryEntry {
                        task_name: "ruvic probe".to_string(),
                        elapsed_ms,
                        bytes_processed: info.container.size_bytes.unwrap_or(0),
                        chunks: info.streams.len(),
                        detail: info.path.clone(),
                    });
                    self.probed_info = Some(*info);
                }
                GuiEvent::MediaOperationFinished { result, elapsed_ms } => {
                    self.busy = false;
                    self.error_banner = None;
                    let mb_s = if elapsed_ms > 0.0 {
                        (result.output_size_bytes as f64 / (1024.0 * 1024.0))
                            / (elapsed_ms / 1000.0)
                    } else {
                        0.0
                    };
                    self.status_line = format!(
                        "ruvic {:?} tamamlandi: {} B ({:.1} MB/sn, {:.2} ms) -> {}",
                        result.operation, result.output_size_bytes, mb_s, elapsed_ms, result.output
                    );
                    self.telemetry.total_bytes_processed += result.output_size_bytes;
                    self.telemetry.jobs_completed += 1;
                    self.telemetry.history.push(TelemetryEntry {
                        task_name: format!("ruvic {:?}", result.operation),
                        elapsed_ms,
                        bytes_processed: result.output_size_bytes,
                        chunks: 1,
                        detail: result.output.clone(),
                    });
                    self.last_media_op = Some(*result);
                }
                GuiEvent::PlaylistResolved {
                    entries,
                    elapsed_ms,
                } => {
                    self.busy = false;
                    self.error_banner = None;
                    self.status_line = format!(
                        "Playlist cozuldu: {} oge bulundu ({:.1} ms)",
                        entries.len(),
                        elapsed_ms
                    );
                    self.playlist_entries = entries.into_iter().map(|e| (true, e)).collect();
                }
                GuiEvent::PlaylistBatchFinished {
                    succeeded,
                    failed,
                    bytes_total,
                    elapsed_ms,
                } => {
                    self.busy = false;
                    self.error_banner = None;
                    self.status_line = format!(
                        "Playlist tamamlandi: {} basarili, {} hatali, {} B ({:.1} ms)",
                        succeeded, failed, bytes_total, elapsed_ms
                    );
                    self.telemetry.total_bytes_downloaded += bytes_total;
                    self.telemetry.jobs_completed += succeeded;
                    self.telemetry.history.push(TelemetryEntry {
                        task_name: "ruvid playlist batch".to_string(),
                        elapsed_ms,
                        bytes_processed: bytes_total,
                        chunks: succeeded,
                        detail: format!("{succeeded} basarili / {failed} hatali"),
                    });
                }
                GuiEvent::TaskFailed { context, error } => {
                    self.busy = false;
                    self.status_line = format!("Hata ({context})");
                    self.error_banner = Some(format!("{context}: {error}"));
                }
            }
        }
    }

    fn spawn_resolve_source(&mut self) {
        let url = self.url_input.trim().to_string();
        if url.is_empty() {
            self.error_banner = Some("Lutfen gecerli bir URL girin.".to_string());
            return;
        }
        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("Kaynak cozuluyor: {url} ...");
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let start = Instant::now();
            let cfg = RuvidConfig::default();
            let ctx = match ResolveContext::build(
                &cfg.network.user_agent,
                cfg.network.connect_timeout_seconds,
                cfg.download.timeout_seconds,
                None,
                &[],
                false,
                cfg.download.retry_count,
            ) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(GuiEvent::TaskFailed {
                        context: "ResolveContext".to_string(),
                        error: e.to_string(),
                    });
                    return;
                }
            };
            match ResolverRegistry::resolve(&ctx, &url).await {
                Ok(mut source) => {
                    redact_media_source(&mut source);
                    let _ = tx.send(GuiEvent::SourceResolved {
                        source: Box::new(source),
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                Err(e) => {
                    let _ = tx.send(GuiEvent::TaskFailed {
                        context: "Akis Cozumleme".to_string(),
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn spawn_download(&mut self) {
        let job = self.build_download_job();
        if job.url.is_empty() {
            self.error_banner = Some("Lutfen indirilecek URL'yi girin.".to_string());
            return;
        }
        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("Indiriliyor ({} --jobs {})...", job.format_spec, job.jobs);
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let start = Instant::now();
            let cfg = RuvidConfig::default();
            match execute_download(&cfg, job, false).await {
                Ok(outcome) => {
                    let _ = tx.send(GuiEvent::DownloadFinished {
                        outcome,
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                Err(e) => {
                    let _ = tx.send(GuiEvent::TaskFailed {
                        context: "Indirme".to_string(),
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn spawn_probe_media(&mut self) {
        let path_str = self.media_input_path.trim().to_string();
        if path_str.is_empty() {
            self.error_banner = Some("Lutfen yoklanacak medya dosyasi yolunu girin.".to_string());
            return;
        }
        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("Yoklaniyor: {path_str} ...");
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let start = Instant::now();
            match probe_media_file(std::path::Path::new(&path_str)) {
                Ok(probed) => {
                    let _ = tx.send(GuiEvent::MediaProbed {
                        info: Box::new(probed.info),
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                Err(e) => {
                    let _ = tx.send(GuiEvent::TaskFailed {
                        context: "Medya Yoklama (Probe)".to_string(),
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn spawn_media_operation(&mut self) {
        let input = PathBuf::from(self.media_input_path.trim());
        let output = PathBuf::from(self.media_output_path.trim());
        if input.as_os_str().is_empty() || output.as_os_str().is_empty() {
            self.error_banner = Some("Lutfen girdi ve cikti dosya yollarini belirtin.".to_string());
            return;
        }
        let secondary = if self.media_secondary_audio_path.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(self.media_secondary_audio_path.trim()))
        };
        let op = self.media_op;
        let trim_start = self.trim_start.trim().to_string();
        let trim_dur = self.trim_duration.max(0.1);
        let thumb_at = self.thumbnail_at.trim().to_string();

        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("ruvic Native Motor calisiyor: {} ...", op.label());
        let tx = self.tx.clone();

        self.runtime.spawn(async move {
            let start = Instant::now();
            let backend = NativeMediaBackend::new();
            let config = RuvicConfig::default();
            let res = match op {
                MediaOperationKind::RemuxFaststart => {
                    execute_remux(
                        &backend,
                        &config,
                        RemuxRequest {
                            input,
                            secondary_input: secondary,
                            output,
                            overwrite: true,
                            video_stream: None,
                            audio_stream: None,
                        },
                        false,
                    )
                    .await
                }
                MediaOperationKind::TrimRange => {
                    execute_trim(
                        &backend,
                        &config,
                        TrimRequest {
                            input,
                            output,
                            start: trim_start,
                            end: None,
                            duration: Some(trim_dur),
                            reencode: false,
                            overwrite: true,
                        },
                        false,
                    )
                    .await
                }
                MediaOperationKind::ExtractAudioAac => {
                    execute_extract_audio(
                        &backend,
                        &config,
                        ExtractAudioRequest {
                            input,
                            output,
                            to: Some("aac".to_string()),
                            copy: true,
                            overwrite: true,
                            audio_stream: None,
                        },
                        false,
                    )
                    .await
                }
                MediaOperationKind::ExtractAudioM4a => {
                    execute_extract_audio(
                        &backend,
                        &config,
                        ExtractAudioRequest {
                            input,
                            output,
                            to: Some("m4a".to_string()),
                            copy: true,
                            overwrite: true,
                            audio_stream: None,
                        },
                        false,
                    )
                    .await
                }
                MediaOperationKind::Thumbnail => {
                    execute_thumbnail(
                        &backend,
                        &config,
                        ThumbnailRequest {
                            input,
                            output,
                            at: thumb_at,
                            overwrite: true,
                        },
                        false,
                    )
                    .await
                }
            };

            match res {
                Ok(result) => {
                    let _ = tx.send(GuiEvent::MediaOperationFinished {
                        result: Box::new(result),
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                Err(e) => {
                    let _ = tx.send(GuiEvent::TaskFailed {
                        context: op.label().to_string(),
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn spawn_resolve_playlist(&mut self) {
        let url = self.playlist_url.trim().to_string();
        if url.is_empty() {
            self.error_banner =
                Some("Lutfen bir Playlist, Podcast RSS veya M3U/JSON adresi girin.".to_string());
            return;
        }
        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("Playlist cozuluyor: {url} ...");
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let start = Instant::now();
            let cfg = RuvidConfig::default();
            let Ok(ctx) = ResolveContext::build(
                &cfg.network.user_agent,
                cfg.network.connect_timeout_seconds,
                cfg.download.timeout_seconds,
                None,
                &[],
                false,
                cfg.download.retry_count,
            ) else {
                return;
            };
            match fetch_playlist_entries(&ctx, &url).await {
                Ok(entries) => {
                    let _ = tx.send(GuiEvent::PlaylistResolved {
                        entries,
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                Err(e) => {
                    let _ = tx.send(GuiEvent::TaskFailed {
                        context: "Playlist Cozumleme".to_string(),
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn spawn_download_playlist(&mut self) {
        let selected: Vec<PlaylistEntry> = self
            .playlist_entries
            .iter()
            .filter(|(checked, _)| *checked)
            .map(|(_, e)| e.clone())
            .collect();
        if selected.is_empty() {
            self.error_banner = Some("Indirilecek en az bir playlist ogesi secin.".to_string());
            return;
        }
        let out_dir = PathBuf::from(self.playlist_output_dir.trim());
        let jobs = self.jobs_count.max(1);
        let skip_errors = self.playlist_skip_errors;
        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("Playlist indiriliyor ({} oge)...", selected.len());
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let start = Instant::now();
            let _ = std::fs::create_dir_all(&out_dir);
            let cfg = RuvidConfig::default();
            let mut succeeded = 0usize;
            let mut failed = 0usize;
            let mut bytes_total = 0u64;

            for entry in selected {
                let title = entry
                    .title
                    .as_deref()
                    .unwrap_or("item")
                    .replace([':', '?', '/', '\\', '*', '"', '<', '>', '|'], "_");
                let template = out_dir.join(format!("{:03}_{title}.%(ext)s", entry.index));
                let job = DownloadJob {
                    url: entry.url,
                    output_template: Some(template),
                    format_spec: "best".to_string(),
                    jobs,
                    resume: true,
                    overwrite: true,
                    limit_rate: None,
                    retry_count: 2,
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
                match execute_download(&cfg, job, false).await {
                    Ok(out) => {
                        succeeded += 1;
                        bytes_total += out.bytes_downloaded;
                    }
                    Err(e) => {
                        failed += 1;
                        if !skip_errors {
                            let _ = tx.send(GuiEvent::TaskFailed {
                                context: "Playlist Indirme".to_string(),
                                error: e.to_string(),
                            });
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(GuiEvent::PlaylistBatchFinished {
                succeeded,
                failed,
                bytes_total,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            });
        });
    }
}

impl eframe::App for RuviStudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_initialized {
            ctx.set_visuals(self.current_theme.visuals());
            self.theme_initialized = true;
        }

        self.poll_events();
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }

        // Top Navigation Header
        egui::TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("⚡ RuviStudio")
                        .color(self.current_theme.accent_primary())
                        .strong(),
                );
                ui.label(
                    RichText::new("Saf Rust Video İndirici (ruvid) & Native Medya Motoru (ruvic)")
                        .color(self.current_theme.text_muted()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (toggle_icon, toggle_name) = match self.current_theme {
                        StudioTheme::Dark => ("🌙", "Koyu"),
                        StudioTheme::Light => ("☀️", "Açık"),
                    };
                    if ui
                        .button(RichText::new(format!("{toggle_icon} {toggle_name}")).strong())
                        .clicked()
                    {
                        self.current_theme = match self.current_theme {
                            StudioTheme::Dark => StudioTheme::Light,
                            StudioTheme::Light => StudioTheme::Dark,
                        };
                        ctx.set_visuals(self.current_theme.visuals());
                    }
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Tamamlanan İş: {} | İndirilen: {:.2} MB",
                            self.telemetry.jobs_completed,
                            self.telemetry.total_bytes_downloaded as f64 / (1024.0 * 1024.0)
                        ))
                        .color(self.current_theme.accent_secondary()),
                    );
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::Downloader,
                    "📥 1. Video İndirici (YouTube / DASH / HLS)",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::MediaStudio,
                    "🎬 2. Medya Stüdyosu (ruvic Native Motor)",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::PlaylistQueue,
                    "📚 3. Playlist & Podcast Kuyruğu",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::Telemetry,
                    "📊 4. Performans & Bellek Monitörü",
                );
            });
            ui.add_space(4.0);
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                }
                ui.label(
                    RichText::new(&self.status_line).color(self.current_theme.status_text()),
                );
            });
        });

        // Main Content Area
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(ref err) = self.error_banner.clone() {
                egui::Frame::NONE
                    .fill(self.current_theme.error_banner_fill())
                    .stroke(Stroke::new(1.0_f32, self.current_theme.error_banner_stroke()))
                    .inner_margin(8.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("⚠ {err}"))
                                    .color(self.current_theme.error_banner_text()),
                            );
                            if ui.small_button("Kapat").clicked() {
                                self.error_banner = None;
                            }
                        });
                    });
                ui.add_space(6.0);
            }

            ScrollArea::vertical().show(ui, |ui| match self.active_tab {
                ActiveTab::Downloader => self.render_downloader_tab(ui),
                ActiveTab::MediaStudio => self.render_media_studio_tab(ui),
                ActiveTab::PlaylistQueue => self.render_playlist_tab(ui),
                ActiveTab::Telemetry => self.render_telemetry_tab(ui),
            });
        });
    }
}

impl RuviStudioApp {
    fn render_downloader_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        ui.heading("📥 YouTube / DASH 4K-8K / HLS / HTTP İndirici");
        ui.add_space(4.0);

        // Card 1: URL & Stream Resolution
        theme.render_card(ui, Some("🎯 Hedef Medya URL'si & Akış Çözümleme"), |ui| {
            ui.horizontal(|ui| {
                ui.label("Medya / YouTube URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.url_input)
                        .desired_width(460.0)
                        .hint_text("https://www.youtube.com/watch?v=..."),
                );
                if ui
                    .add_enabled(!self.busy, egui::Button::new("🔍 Akışları Çözümle (Info)"))
                    .clicked()
                {
                    self.spawn_resolve_source();
                }
            });
            ui.add_space(2.0);
            ui.label(
                RichText::new("Desteklenen: YouTube (Innertube iOS/DASH), HLS .m3u8, DASH .mpd, Direkt HTTP/HTTPS")
                    .size(11.5)
                    .color(theme.text_muted()),
            );
        });
        ui.add_space(8.0);

        // Card 2: Format & Download Options
        theme.render_card(ui, Some("⚙️ Format, Kalite & Paralel İndirme Seçenekleri"), |ui| {
            egui::Grid::new("dl_format_opts_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Çıktı Şablonu / Dosya:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.output_path_input)
                            .desired_width(460.0)
                            .hint_text("./downloads/%(title)s.%(ext)s"),
                    );
                    ui.end_row();

                    ui.label("Format / Çözünürlük:");
                    ui.horizontal(|ui| {
                        for preset in [
                            "best", "4320p", "2160p", "1080p", "720p", "360p", "240p", "worst",
                        ] {
                            ui.selectable_value(&mut self.format_spec, preset.to_string(), preset);
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.format_spec)
                                .desired_width(90.0)
                                .hint_text("itag-137"),
                        );
                    });
                    ui.end_row();

                    ui.label("Paralel Range Parçaları (--jobs):");
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(&mut self.jobs_count, 1..=16).suffix(" parça"));
                        ui.checkbox(&mut self.resume_enabled, "Resume Checkpoint");
                        ui.checkbox(&mut self.overwrite_enabled, "Üzerine Yaz (--overwrite)");
                    });
                    ui.end_row();
                });
        });
        ui.add_space(8.0);

        // Card 3: Subtitles, Audio & Clip Settings
        theme.render_card(ui, Some("✂️ Altyazı, Dublaj Ses & Klip Parametreleri"), |ui| {
            egui::Grid::new("dl_sub_clip_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Altyazı & Dublaj Ses:");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.download_subtitles, "Altyazıları İndir");
                        ui.checkbox(&mut self.auto_subtitles, "Otomatik (ASR)");
                        ui.label("Format:");
                        for fmt in ["srt", "vtt", "sbv"] {
                            ui.selectable_value(&mut self.subtitle_format, fmt.to_string(), fmt);
                        }
                        ui.label("Altyazı Dili:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.subtitle_lang)
                                .desired_width(48.0)
                                .hint_text("tr/en"),
                        );
                        ui.label("Ses Dili (--audio-lang):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.audio_lang)
                                .desired_width(48.0)
                                .hint_text("tr/en"),
                        );
                    });
                    ui.end_row();

                    ui.label("Bölümler & Klip Aralığı:");
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut self.split_chapters,
                            "Bölümleri Ayrı Dosyalara Böl (--split-chapters)",
                        );
                        ui.label("Başlangıç:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.clip_start)
                                .desired_width(68.0)
                                .hint_text("00:00:10"),
                        );
                        ui.label("Bitiş:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.clip_end)
                                .desired_width(68.0)
                                .hint_text("00:00:25"),
                        );
                        ui.label("Süre (sn):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.clip_duration)
                                .desired_width(50.0)
                                .hint_text("15"),
                        );
                    });
                    ui.end_row();
                });
        });
        ui.add_space(10.0);

        // Primary Action Button
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(
                        RichText::new("🚀 Paralel İndirmeyi Başlat (ruvid + Otomatik DASH Remux)")
                            .size(16.0)
                            .strong(),
                    )
                    .min_size(Vec2::new(380.0, 38.0)),
                )
                .clicked()
            {
                self.spawn_download();
            }
        });
        ui.add_space(10.0);

        // RuviTube Setup Card
        self.render_ruvitube_setup_card(ui);

        // Dynamic Outcome Card
        if let Some(outcome) = self.last_download.clone() {
            ui.add_space(8.0);
            theme.render_card(ui, Some("✅ Son İndirme Özeti"), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} [{}] — {} bayt ({} Range parçası) -> {}",
                        outcome.title,
                        outcome.stream_id,
                        outcome.bytes_downloaded,
                        outcome.chunks_total,
                        outcome.output_path
                    ))
                    .color(theme.accent_primary())
                    .strong(),
                );
                if !outcome.subtitles.is_empty() {
                    ui.add_space(4.0);
                    ui.label(format!(
                        "📝 İndirilen Altyazılar ({}): {}",
                        outcome.subtitles.len(),
                        outcome
                            .subtitles
                            .iter()
                            .map(|s| format!("{} ({})", s.language, s.path))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !outcome.chapters.is_empty() {
                    ui.add_space(4.0);
                    ui.label(format!(
                        "✂ Bölünen Chapter Dosyaları ({}): {}",
                        outcome.chapters.len(),
                        outcome.chapters.join(", ")
                    ));
                }
            });
        }

        // Dynamic Streams Card
        if let Some(source) = self.resolved_source.clone() {
            ui.add_space(8.0);
            let title_str = format!("📺 Çözülen Kaynak: {}", source.title);
            theme.render_card(ui, Some(&title_str), |ui| {
                ui.label(format!(
                    "Akış Sayısı: {} | Altyazı: {} | Bölümler (Chapters): {}",
                    source.streams.len(),
                    source.subtitles.len(),
                    source.chapters.len()
                ));
                ui.add_space(6.0);

                egui::Grid::new("resolved_streams_grid")
                    .num_columns(7)
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Seç");
                        ui.strong("Akış ID");
                        ui.strong("Tür");
                        ui.strong("Kapsayıcı");
                        ui.strong("Çözünürlük / FPS");
                        ui.strong("Kodek & Dil");
                        ui.strong("Bit Hızı");
                        ui.end_row();

                        for s in &source.streams {
                            if ui.small_button("Kullan").clicked() {
                                self.format_spec = s.id.clone();
                            }
                            ui.label(&s.id);
                            ui.label(format!("{:?}", s.kind));
                            ui.label(&s.container);
                            let res = match (s.width, s.height) {
                                (Some(w), Some(h)) => {
                                    format!(
                                        "{}x{} ({}fps)",
                                        w,
                                        h,
                                        s.frame_rate.as_deref().unwrap_or("30")
                                    )
                                }
                                _ => "-".to_string(),
                            };
                            ui.label(res);
                            ui.label(format!(
                                "{}{}",
                                s.codecs.as_deref().unwrap_or("-"),
                                s.language
                                .as_deref()
                                .map(|l| format!(" [{l}]"))
                                .unwrap_or_default()
                            ));
                            ui.label(
                                s.bandwidth_bps
                                    .map(|b| format!("{} kbps", b / 1000))
                                    .unwrap_or_else(|| "-".to_string()),
                            );
                            ui.end_row();
                        }
                    });
            });
        }
    }

    fn render_media_studio_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        ui.heading("🎬 ruvic Saf Rust Native Medya Motoru (Sıfır FFmpeg Bağımlılığı)");
        ui.label(
            RichText::new(
                "Seek tabanlı 64KB/1MB Streaming I/O · MP4 stco/co64 Offset Rebasing · ADTS AAC & M4A Demux · 360/VR & HDR Koruma",
            )
            .color(theme.text_muted()),
        );
        ui.add_space(8.0);

        // Card 1: Input & Probe
        theme.render_card(ui, Some("📁 Girdi Medyası & Akış Yoklama (Probe)"), |ui| {
            egui::Grid::new("media_studio_input_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Girdi Medya Dosyası:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.media_input_path)
                                .desired_width(440.0)
                                .hint_text("./downloads/video.mp4"),
                        );
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("🔍 Yokla (Probe)"))
                            .clicked()
                        {
                            self.spawn_probe_media();
                        }
                    });
                    ui.end_row();

                    ui.label("İkincil Ses Dosyası (DASH Mux):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.media_secondary_audio_path)
                            .desired_width(440.0)
                            .hint_text("Boş bırakılabilir veya ayrık ses dosyası (.m4a / .webm)"),
                    );
                    ui.end_row();
                });
        });
        ui.add_space(8.0);

        // Card 2: Operation & Parameters
        theme.render_card(ui, Some("⚡ ruvic Native İşlem Seçimi & Parametreler"), |ui| {
            egui::Grid::new("media_studio_op_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("İşlem Türü:");
                    ui.horizontal_wrapped(|ui| {
                        for kind in [
                            MediaOperationKind::RemuxFaststart,
                            MediaOperationKind::TrimRange,
                            MediaOperationKind::ExtractAudioAac,
                            MediaOperationKind::ExtractAudioM4a,
                            MediaOperationKind::Thumbnail,
                        ] {
                            ui.selectable_value(&mut self.media_op, kind, kind.label());
                        }
                    });
                    ui.end_row();

                    if self.media_op == MediaOperationKind::TrimRange {
                        ui.label("Trim Başlangıç & Süre (sn):");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.trim_start).desired_width(90.0),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.trim_duration, 0.5..=600.0).suffix(" sn"),
                            );
                        });
                        ui.end_row();
                    } else if self.media_op == MediaOperationKind::Thumbnail {
                        ui.label("Küçük Resim Zamanı (--at):");
                        ui.add(egui::TextEdit::singleline(&mut self.thumbnail_at).desired_width(90.0));
                        ui.end_row();
                    }

                    ui.label("Çıktı Dosya Yolu:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.media_output_path)
                            .desired_width(440.0)
                            .hint_text("./downloads/output.mp4"),
                    );
                    ui.end_row();
                });
        });
        ui.add_space(10.0);

        // Action button
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(
                    RichText::new("⚡ Saf Rust Native Motor İle İşle")
                        .size(16.0)
                        .strong(),
                )
                .min_size(Vec2::new(320.0, 38.0)),
            )
            .clicked()
        {
            self.spawn_media_operation();
        }

        // Dynamic Result Card
        if let Some(res) = self.last_media_op.clone() {
            ui.add_space(10.0);
            theme.render_card(ui, Some("🎉 İşlem Tamamlandı"), |ui| {
                ui.label(
                    RichText::new(format!(
                        "Çıktı: {} — {} bayt ({} ms)",
                        res.output,
                        res.output_size_bytes,
                        res.duration_ms
                    ))
                    .color(theme.accent_primary())
                    .strong(),
                );
            });
        }

        // Dynamic Probed Info Card
        if let Some(info) = self.probed_info.clone() {
            ui.add_space(10.0);
            let info_title = format!(
                "🔍 Konteyner Analizi: {} ({})",
                info.container.format_name,
                info.container.format_long_name.as_deref().unwrap_or("-")
            );
            theme.render_card(ui, Some(&info_title), |ui| {
                ui.label(format!(
                    "Süre: {:.2} sn | Boyut: {} B | Bit Hızı: {} bps",
                    info.container.duration_seconds.unwrap_or(0.0),
                    info.container.size_bytes.unwrap_or(0),
                    info.container.bit_rate.unwrap_or(0)
                ));
                ui.add_space(4.0);
                for s in &info.streams {
                    let hdr_badge = if s.color_primaries.is_some() || s.mastering_display.is_some() {
                        " [HDR colr/mdcv]"
                    } else {
                        ""
                    };
                    let vr_badge = if s.spatial_360.is_some() {
                        " [360° VR sv3d]"
                    } else {
                        ""
                    };
                    ui.label(format!(
                        "  • Stream #{}: {:?} | Kodek: {} | {:?}x{:?} | {:?} Hz{hdr_badge}{vr_badge}",
                        s.index, s.kind, s.codec_name, s.width, s.height, s.sample_rate
                    ));
                }
            });
        }
    }

    fn render_playlist_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        ui.heading("📚 Toplu Playlist, Dizi ve Podcast İndirici");
        ui.add_space(6.0);

        // Card 1: Playlist & RSS Config
        theme.render_card(ui, Some("📚 Playlist / RSS / M3U Kaynak Ayarları"), |ui| {
            ui.horizontal(|ui| {
                ui.label("Playlist / RSS / M3U URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.playlist_url)
                        .desired_width(440.0)
                        .hint_text("https://www.youtube.com/playlist?list=..."),
                );
                if ui
                    .add_enabled(!self.busy, egui::Button::new("📋 Listeyi Çözümle"))
                    .clicked()
                {
                    self.spawn_resolve_playlist();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Çıktı Klasörü:");
                ui.add(egui::TextEdit::singleline(&mut self.playlist_output_dir).desired_width(260.0));
                ui.add(
                    egui::Slider::new(&mut self.playlist_concurrency, 1..=8).text("Eşzamanlı İndirme"),
                );
                ui.checkbox(
                    &mut self.playlist_skip_errors,
                    "Hataları Atla (--skip-errors)",
                );
            });
        });
        ui.add_space(8.0);

        // Card 2: Queued Items & Batch Download
        if !self.playlist_entries.is_empty() {
            theme.render_card(ui, Some("📋 Kuyruktaki İçerikler & Toplu İndirme"), |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(
                                RichText::new("📥 Seçilen Öğeleri İndir")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(theme.accent_primary())
                            .min_size(Vec2::new(200.0, 32.0)),
                        )
                        .clicked()
                    {
                        self.spawn_download_playlist();
                    }

                    if ui.button("Tümünü Seç").clicked() {
                        for (checked, _) in &mut self.playlist_entries {
                            *checked = true;
                        }
                    }
                    if ui.button("Seçimi Kaldır").clicked() {
                        for (checked, _) in &mut self.playlist_entries {
                            *checked = false;
                        }
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                for (checked, entry) in &mut self.playlist_entries {
                    ui.checkbox(
                        checked,
                        format!(
                            "#{:03} — {} ({})",
                            entry.index,
                            entry.title.as_deref().unwrap_or("Başlıksız"),
                            entry.url
                        ),
                    );
                }
            });
        }
    }

    fn render_telemetry_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        ui.heading("📊 Performans, Bellek Yükü ve Piyasa Kıyaslama Panosu");
        ui.add_space(6.0);

        ui.columns(3, |cols| {
            theme.render_card(&mut cols[0], Some("UYGULAMA BOYUTU (SIFIR BAĞIMLILIK)"), |ui| {
                ui.heading(
                    RichText::new("ruvid: 6.87 MB | ruvic: 3.73 MB")
                        .color(theme.accent_primary()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Piyasa Yığını (yt-dlp + deno + ffmpeg): ~360 MB (~34x daha küçük)")
                        .color(theme.text_muted()),
                );
            });

            theme.render_card(&mut cols[1], Some("50.3 MB MP4 STREAMING BELLEK"), |ui| {
                ui.heading(
                    RichText::new("3.53 MB Özel RAM (42.36 ms)")
                        .color(theme.accent_secondary()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Aktarım Hızı: ~1,187 MB/sn (PeakWorkingSet: 8.98 MB — %7)")
                        .color(theme.text_muted()),
                );
            });

            theme.render_card(&mut cols[2], Some("SİSTEM BÜTÜNLÜĞÜ & TESTLER"), |ui| {
                ui.heading(
                    RichText::new("209+ Test Yeşil | Exit 8 Koruma")
                        .color(theme.accent_purple()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Atomik .part/.ruvic-tmp yazım + validate_moov_integrity")
                        .color(theme.text_muted()),
                );
            });
        });

        ui.add_space(12.0);
        if self.telemetry.history.is_empty() {
            theme.render_card(ui, Some("⏱ Gerçek Zamanlı Oturum İşlem Geçmişi"), |ui| {
                ui.label(
                    RichText::new("Henüz bu oturumda bir indirme veya medya işlemi çalıştırılmadı.")
                        .color(theme.text_muted()),
                );
            });
        } else {
            let mut clear_history = false;
            let history_items = self.telemetry.history.clone();
            theme.render_card(ui, Some("⏱ Gerçek Zamanlı Oturum İşlem Geçmişi"), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Geçmişi Temizle").clicked() {
                        clear_history = true;
                    }
                });
                ui.add_space(4.0);

                egui::Grid::new("telemetry_history_grid")
                    .num_columns(5)
                    .striped(true)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("İşlem");
                        ui.strong("Süre (ms)");
                        ui.strong("İşlenen Boyut");
                        ui.strong("Parça / Akış");
                        ui.strong("Detay / Çıktı");
                        ui.end_row();

                        for item in history_items.iter().rev() {
                            ui.label(&item.task_name);
                            ui.label(format!("{:.2} ms", item.elapsed_ms));
                            ui.label(format!("{} B", item.bytes_processed));
                            ui.label(format!("{}", item.chunks));
                            ui.label(&item.detail);
                            ui.end_row();
                        }
                    });
            });
            if clear_history {
                self.telemetry.history.clear();
            }
        }

        ui.add_space(8.0);
        self.render_ruvitube_setup_card(ui);
    }
}

pub trait UiSubheadingExt {
    fn subheading_bar(&mut self, text: &str, theme: StudioTheme);
}

impl UiSubheadingExt for egui::Ui {
    fn subheading_bar(&mut self, text: &str, theme: StudioTheme) {
        self.label(
            RichText::new(text)
                .size(15.0)
                .color(theme.accent_secondary())
                .strong(),
        );
        self.separator();
    }
}
