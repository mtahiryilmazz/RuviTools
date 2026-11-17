use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use eframe::egui::{self, Color32, Rect, RichText, ScrollArea, Stroke, Vec2};
use ruvic::backend::native::NativeMediaBackend;
use ruvic::backend::native::container_probe::probe_media_file;
use ruvic::config::RuvicConfig;
use ruvic::domain::conversion_job::OperationResult;
use ruvic::domain::media_info::MediaInfo;
use ruvic::operations::convert::{ConvertRequest, execute_convert};
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

use crate::icons;

/// Registers guarded Windows fallback fonts (`segoeui.ttf`, `seguisym.ttf`) if present
/// and valid via `ab_glyph::FontRef::try_from_slice`, safely no-oping when absent.
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        ("segoeui", r"C:\Windows\Fonts\segoeui.ttf"),
        ("seguisym", r"C:\Windows\Fonts\seguisym.ttf"),
    ];
    let mut added = false;

    for (name, path_str) in candidates {
        let path = Path::new(path_str);
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(path)
            && ab_glyph::FontRef::try_from_slice(&bytes).is_ok()
        {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push(name.to_owned());
            }
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.push(name.to_owned());
            }
            added = true;
        }
    }

    if added {
        ctx.set_fonts(fonts);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Downloader,
    MediaStudio,
    PlaylistQueue,
    Telemetry,
    RuviTube,
    Settings,
}

impl ActiveTab {
    pub const PRIMARY_TABS: [Self; 5] = [
        Self::Downloader,
        Self::MediaStudio,
        Self::PlaylistQueue,
        Self::Telemetry,
        Self::RuviTube,
    ];

    pub const ALL_TABS: [Self; 6] = [
        Self::Downloader,
        Self::MediaStudio,
        Self::PlaylistQueue,
        Self::Telemetry,
        Self::RuviTube,
        Self::Settings,
    ];

    pub fn primary_tabs() -> &'static [Self] {
        &Self::PRIMARY_TABS
    }

    pub fn all() -> &'static [Self] {
        &Self::ALL_TABS
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Downloader => "DL",
            Self::MediaStudio => "MS",
            Self::PlaylistQueue => "PQ",
            Self::Telemetry => "TM",
            Self::RuviTube => "RT",
            Self::Settings => "ST",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Downloader => "İndirici",
            Self::MediaStudio => "Medya Stüdyosu",
            Self::PlaylistQueue => "Playlist Kuyruğu",
            Self::Telemetry => "Telemetri & Performans",
            Self::RuviTube => "RuviTube Eklentisi",
            Self::Settings => "Ayarlar",
        }
    }

    pub fn title(self) -> &'static str {
        self.label()
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Downloader => "YouTube, DASH & HLS",
            Self::MediaStudio => "ruvic Native Remux & Trim",
            Self::PlaylistQueue => "Toplu İndirme & Podcast",
            Self::Telemetry => "Hız, Bellek & Geçmiş",
            Self::RuviTube => "Chrome & NativeHost",
            Self::Settings => "Tema, İndirme & Motor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOperationKind {
    ConvertVideo,
    RemuxFaststart,
    TrimRange,
    ExtractAudioAac,
    ExtractAudioM4a,
    Thumbnail,
}

impl MediaOperationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ConvertVideo => "Video Dönüştürme (Convert / Transcode)",
            Self::RemuxFaststart => "Faststart Remux (MP4 / WebM)",
            Self::TrimRange => "Zaman Araligi Kesme (Trim)",
            Self::ExtractAudioAac => "Ses Cikarimi (ADTS .aac)",
            Self::ExtractAudioM4a => "Ses Cikarimi (Kapsayici .m4a)",
            Self::Thumbnail => "Kucuk Resim Yakala (Thumbnail)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StudioTheme {
    #[default]
    Dark,
    Light,
}

impl StudioTheme {
    pub fn visuals(&self) -> egui::Visuals {
        match self {
            Self::Dark => {
                let mut visuals = egui::Visuals::dark();
                visuals.panel_fill = Color32::from_rgb(13, 17, 23); // #0d1117
                visuals.window_fill = Color32::from_rgb(22, 27, 34); // #161b22
                visuals.extreme_bg_color = Color32::from_rgb(9, 13, 19); // #090d13
                visuals.faint_bg_color = Color32::from_rgb(22, 27, 34);
                visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 27, 34);
                visuals.widgets.noninteractive.bg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(48, 54, 61)); // #30363d
                visuals.widgets.noninteractive.fg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(201, 209, 217)); // #c9d1d9
                visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 38, 45); // #21262d
                visuals.widgets.inactive.bg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(48, 54, 61));
                visuals.widgets.inactive.fg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(201, 209, 217));
                visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 54, 61);
                visuals.widgets.hovered.bg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(88, 166, 255)); // #58a6ff
                visuals.widgets.active.bg_fill = Color32::from_rgb(31, 111, 235); // #1f6feb
                visuals.selection.bg_fill = Color32::from_rgb(31, 111, 235);
                visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(88, 166, 255));
                visuals
            }
            Self::Light => {
                let mut visuals = egui::Visuals::light();
                visuals.panel_fill = Color32::from_rgb(246, 248, 250); // #f6f8fa
                visuals.window_fill = Color32::from_rgb(255, 255, 255); // #ffffff
                visuals.extreme_bg_color = Color32::from_rgb(241, 245, 249); // #f1f5f9
                visuals.faint_bg_color = Color32::from_rgb(246, 248, 250);
                visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(255, 255, 255);
                visuals.widgets.noninteractive.bg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(208, 215, 222)); // #d0d7de
                visuals.widgets.noninteractive.fg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(36, 41, 47)); // #24292f
                visuals.widgets.inactive.bg_fill = Color32::from_rgb(234, 238, 242); // #eaeef2
                visuals.widgets.inactive.bg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(208, 215, 222));
                visuals.widgets.inactive.fg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(36, 41, 47));
                visuals.widgets.hovered.bg_fill = Color32::from_rgb(220, 226, 232);
                visuals.widgets.hovered.bg_stroke =
                    Stroke::new(1.0_f32, Color32::from_rgb(9, 105, 218)); // #0969da
                visuals.widgets.active.bg_fill = Color32::from_rgb(9, 105, 218);
                visuals.selection.bg_fill = Color32::from_rgb(9, 105, 218);
                visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(9, 105, 218));
                visuals
            }
        }
    }

    pub fn surface_fill(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(22, 27, 34), // #161b22
            Self::Light => Color32::from_rgb(255, 255, 255), // #ffffff
        }
    }

    pub fn border_color(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(48, 54, 61), // #30363d
            Self::Light => Color32::from_rgb(208, 215, 222), // #d0d7de
        }
    }

    pub fn card_frame(&self) -> egui::Frame {
        egui::Frame::NONE
            .fill(self.surface_fill())
            .stroke(Stroke::new(1.0_f32, self.border_color()))
            .inner_margin(14.0)
            .corner_radius(8.0)
    }

    pub fn sidebar_frame(&self) -> egui::Frame {
        egui::Frame::NONE
            .fill(self.surface_fill())
            .stroke(Stroke::NONE)
            .inner_margin(egui::Margin::symmetric(10, 12))
    }

    pub fn titlebar_frame(&self) -> egui::Frame {
        egui::Frame::NONE
            .fill(self.surface_fill())
            .stroke(Stroke::NONE)
            .inner_margin(egui::Margin::symmetric(14, 0))
    }

    pub fn nav_item_active_fill(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(33, 38, 45), // #21262d
            Self::Light => Color32::from_rgb(234, 238, 242), // #eaeef2
        }
    }

    pub fn nav_item_hover_fill(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(28, 33, 40), // #1c2128
            Self::Light => Color32::from_rgb(243, 244, 246), // #f3f4f6
        }
    }

    pub fn brand_fill(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(31, 111, 235), // #1f6feb
            Self::Light => Color32::from_rgb(9, 105, 218), // #0969da
        }
    }

    pub fn accent_primary(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(63, 185, 80), // #3fb950
            Self::Light => Color32::from_rgb(26, 127, 55), // #1a7f37
        }
    }

    pub fn success_cta_fill(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(35, 134, 54), // #238636
            Self::Light => Color32::from_rgb(31, 136, 61), // #1f883d
        }
    }

    pub fn accent_secondary(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(88, 166, 255), // #58a6ff
            Self::Light => Color32::from_rgb(9, 105, 218), // #0969da
        }
    }

    pub fn accent_purple(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(168, 85, 247), // #a855f7
            Self::Light => Color32::from_rgb(130, 80, 223), // #8250df
        }
    }

    pub fn status_ready_colors(&self) -> (Color32, Color32, Color32) {
        match self {
            Self::Dark => (
                Color32::from_rgb(13, 42, 20),  // #0d2a14
                Color32::from_rgb(35, 134, 54), // #238636
                Color32::from_rgb(63, 185, 80), // #3fb950
            ),
            Self::Light => (
                Color32::from_rgb(218, 251, 225), // #dafbe1
                Color32::from_rgb(74, 194, 107),  // #4ac26b
                Color32::from_rgb(26, 127, 55),   // #1a7f37
            ),
        }
    }

    pub fn status_busy_colors(&self) -> (Color32, Color32, Color32) {
        match self {
            Self::Dark => (
                Color32::from_rgb(42, 31, 13),   // #2a1f0d
                Color32::from_rgb(158, 106, 3),  // #9e6a03
                Color32::from_rgb(210, 153, 34), // #d29922
            ),
            Self::Light => (
                Color32::from_rgb(255, 248, 197), // #fff8c5
                Color32::from_rgb(212, 167, 44),  // #d4a72c
                Color32::from_rgb(154, 103, 0),   // #9a6700
            ),
        }
    }

    pub fn status_error_colors(&self) -> (Color32, Color32, Color32) {
        match self {
            Self::Dark => (
                Color32::from_rgb(42, 13, 13),  // #2a0d0d
                Color32::from_rgb(218, 54, 51), // #da3633
                Color32::from_rgb(248, 81, 73), // #f85149
            ),
            Self::Light => (
                Color32::from_rgb(255, 235, 233), // #ffebe9
                Color32::from_rgb(255, 129, 130), // #ff8182
                Color32::from_rgb(207, 34, 46),   // #cf222e
            ),
        }
    }

    pub fn text_muted(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(139, 148, 158), // #8b949e
            Self::Light => Color32::from_rgb(101, 109, 118), // #656d76
        }
    }

    pub fn status_text(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(201, 209, 217), // #c9d1d9
            Self::Light => Color32::from_rgb(36, 41, 47),   // #24292f
        }
    }

    pub fn error_banner_fill(&self) -> Color32 {
        self.status_error_colors().0
    }

    pub fn error_banner_stroke(&self) -> Color32 {
        self.status_error_colors().1
    }

    pub fn error_banner_text(&self) -> Color32 {
        self.status_error_colors().2
    }

    pub fn card_header_text(&self) -> Color32 {
        match self {
            Self::Dark => Color32::from_rgb(240, 246, 252), // #f0f6fc
            Self::Light => Color32::from_rgb(31, 35, 40),   // #1f2328
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
                                .size(14.5)
                                .strong()
                                .color(self.card_header_text()),
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
    pub convert_video_codec: String,
    pub convert_audio_codec: String,
    pub convert_preset: String,
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
    chrome_applied_theme: Option<StudioTheme>,
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
    fn detect_ruvitube_dir() -> String {
        let candidates = [
            "./ruvitube",
            "../ruvitube",
            r"d:\mtahiryilmazz\Github\RuviTools\ruvitube",
            "../GeTube/GeTube_v2.0.0",
            r"d:\mtahiryilmazz\Github\GeTube\GeTube_v2.0.0",
        ];
        for candidate in candidates {
            let p = Path::new(candidate);
            if p.join("extension").join("manifest.json").is_file()
                || p.join("bin").join("NativeHost.exe").is_file()
                || p.is_dir()
            {
                return candidate.to_string();
            }
        }
        "./ruvitube".to_string()
    }

    pub fn new(runtime: Arc<tokio::runtime::Runtime>) -> Self {
        let (tx, rx) = channel();
        let default_getube_dir = Self::detect_ruvitube_dir();
        let mut app = Self {
            active_tab: ActiveTab::Downloader,
            runtime,
            tx,
            rx,
            busy: false,
            status_line:
                "Hazır — Saf Rust ruvid + ruvic motorları bağlı (Sıfır harici bağımlılık)."
                    .to_string(),
            error_banner: None,
            current_theme: StudioTheme::Dark,
            getube_dir: default_getube_dir,
            getube_status:
                "Kuruluma hazır. 'RuviTube Eklentisini Kur' ile Native Host'u kaydedebilirsiniz."
                    .to_string(),
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
            media_output_path: "./downloads/output.mp4".to_string(),
            media_op: MediaOperationKind::ConvertVideo,
            convert_video_codec: "h264".to_string(),
            convert_audio_codec: "aac".to_string(),
            convert_preset: "fast".to_string(),
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
            chrome_applied_theme: None,
        };
        app.check_getube_status();
        app
    }

    pub fn theme(&self) -> StudioTheme {
        self.current_theme
    }

    pub fn set_theme(&mut self, theme: StudioTheme) {
        self.current_theme = theme;
        self.theme_initialized = false;
    }

    pub fn render_headless(&mut self, ctx: &egui::Context) {
        self.render_ui(ctx);
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
            for alt in [
                "./ruvitube",
                "../ruvitube",
                r"d:\mtahiryilmazz\Github\RuviTools\ruvitube",
                "../GeTube/GeTube_v2.0.0",
            ] {
                let alt_path = PathBuf::from(alt);
                if alt_path.exists() {
                    base = alt_path;
                    self.getube_dir = base.to_string_lossy().to_string();
                    break;
                }
            }
        }

        let host_bin = base.join("bin").join("NativeHost.exe");
        let ext_manifest = base.join("extension").join("manifest.json");

        self.getube_native_host_found = host_bin.is_file();
        self.getube_extension_found = ext_manifest.is_file();

        if self.getube_native_host_found && self.getube_extension_found {
            self.getube_status =
                "RuviTube Chrome eklentisi ve Native Host hazır durumda.".to_string();
        } else if self.getube_extension_found {
            self.getube_status =
                "RuviTube Chrome eklentisi bulundu. Native Host kurulumu için aşağıdaki butona tıklayın."
                    .to_string();
        } else {
            self.getube_status =
                format!("Belirtilen dizinde eklenti bulunamadı: {}", base.display());
        }
    }

    pub fn install_getube(&mut self) {
        let base = PathBuf::from(self.getube_dir.trim());
        let installer_gui_ps1 = base.join("installer").join("Install-GUI.ps1");
        let installer_cmd = base.join("installer").join("Kurulum.cmd");
        let installer_ps1 = base.join("installer").join("Install.ps1");
        let installer_core_ps1 = base.join("installer").join("install-core.ps1");
        let ext_dir = base.join("extension");

        if !base.exists() {
            self.getube_status = format!("RuviTube dizini bulunamadı: {}", base.display());
            return;
        }

        #[cfg(target_os = "windows")]
        {
            if installer_gui_ps1.is_file() {
                let _ = std::process::Command::new("powershell.exe")
                    .args([
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                        installer_gui_ps1.to_str().unwrap_or_default(),
                    ])
                    .spawn();
                self.getube_status =
                    "RuviTube Install-GUI.ps1 çalıştırıldı. Native Host kaydediliyor..."
                        .to_string();
            } else if installer_cmd.is_file() {
                let _ = std::process::Command::new("cmd.exe")
                    .args(["/C", installer_cmd.to_str().unwrap_or_default()])
                    .spawn();
                self.getube_status = "RuviTube Kurulum.cmd çalıştırıldı.".to_string();
            } else if installer_ps1.is_file() {
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
                    "RuviTube Install.ps1 çalıştırıldı. Native Host kaydediliyor...".to_string();
            } else if installer_core_ps1.is_file() {
                let _ = std::process::Command::new("powershell.exe")
                    .args([
                        "-NoProfile",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                        installer_core_ps1.to_str().unwrap_or_default(),
                    ])
                    .spawn();
                self.getube_status = "RuviTube install-core.ps1 çalıştırıldı.".to_string();
            } else {
                self.getube_status = format!(
                    "Kurulum betiği bulunamadı ({} veya {}).",
                    installer_gui_ps1.display(),
                    installer_cmd.display()
                );
            }

            if ext_dir.is_dir() {
                let _ = std::process::Command::new("explorer.exe")
                    .arg(&ext_dir)
                    .spawn();
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            self.getube_status = format!("RuviTube eklenti dizini: {}", ext_dir.display());
        }

        self.check_getube_status();
        self.status_line = "RuviTube kurulumu tetiklendi. Chrome'da 'chrome://extensions' sayfasından 'Paketlenmemiş öğe yükle' ile eklentiyi aktifleştirebilirsiniz.".to_string();
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

    fn render_page_heading(&self, ui: &mut egui::Ui, tab: ActiveTab, title: &str) {
        let theme = self.current_theme;
        ui.horizontal(|ui| {
            let (icon_rect, _) =
                ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::hover());
            icons::draw_tab_icon(ui.painter(), icon_rect, tab, theme.accent_secondary());
            ui.add_space(4.0);
            ui.heading(
                RichText::new(title)
                    .size(18.5)
                    .strong()
                    .color(theme.card_header_text()),
            );
        });
    }

    pub fn render_ruvitube_tab(&mut self, ui: &mut egui::Ui) {
        self.render_page_heading(
            ui,
            ActiveTab::RuviTube,
            "RuviTube Chrome Eklentisi & Native Host Entegrasyonu",
        );
        ui.add_space(6.0);
        self.render_ruvitube_setup_card(ui);
    }

    pub fn render_ruvitube_setup_card(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        theme.render_card(
            ui,
            Some("RuviTube Chrome Eklentisi & NativeHost Kurulumu"),
            |ui| {
                ui.label(
                    RichText::new(
                        "RuviTube, YouTube web oynatıcısına doğrudan klip kesme, paralel Range (--jobs) ve indirme düğmesi ekleyen Chrome eklentisidir. RuviTools NativeHost.exe ve ruvid motoruyla entegre çalışır.",
                    )
                    .color(theme.text_muted()),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Eklenti / Kurulum Dizini:");
                    let edit_res = ui.add(
                        egui::TextEdit::singleline(&mut self.getube_dir)
                            .desired_width(360.0)
                            .hint_text("./ruvitube"),
                    );
                    if edit_res.changed() {
                        self.check_getube_status();
                    }
                    if ui.button("Yeniden Tara").clicked() {
                        self.check_getube_status();
                    }
                });

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let (host_icon_rect, _) =
                        ui.allocate_exact_size(Vec2::new(14.0, 14.0), egui::Sense::hover());
                    if self.getube_native_host_found {
                        icons::draw_check_icon(
                            ui.painter(),
                            host_icon_rect,
                            theme.accent_primary(),
                        );
                        ui.label(
                            RichText::new("Native Host: Bulundu (bin/NativeHost.exe)")
                                .color(theme.accent_primary()),
                        );
                    } else {
                        icons::draw_cross_icon(
                            ui.painter(),
                            host_icon_rect,
                            theme.error_banner_stroke(),
                        );
                        ui.label(
                            RichText::new("Native Host: Bulunamadı (bin/NativeHost.exe)")
                                .color(theme.error_banner_stroke()),
                        );
                    }

                    ui.separator();

                    let (ext_icon_rect, _) =
                        ui.allocate_exact_size(Vec2::new(14.0, 14.0), egui::Sense::hover());
                    if self.getube_extension_found {
                        icons::draw_check_icon(
                            ui.painter(),
                            ext_icon_rect,
                            theme.accent_primary(),
                        );
                        ui.label(
                            RichText::new("Chrome Eklentisi: Bulundu (extension/manifest.json)")
                                .color(theme.accent_primary()),
                        );
                    } else {
                        icons::draw_cross_icon(
                            ui.painter(),
                            ext_icon_rect,
                            theme.error_banner_stroke(),
                        );
                        ui.label(
                            RichText::new("Chrome Eklentisi: Bulunamadı (extension/manifest.json)")
                                .color(theme.error_banner_stroke()),
                        );
                    }
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("RuviTube Eklentisini Kur / Yapılandır")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(theme.success_cta_fill())
                            .min_size(Vec2::new(260.0, 32.0)),
                        )
                        .clicked()
                    {
                        self.install_getube();
                    }

                    if ui.button("Eklenti Klasörünü Aç (extension)").clicked() {
                        self.open_getube_extension_dir();
                    }

                    if ui
                        .button("Chrome Eklentilerini Aç (chrome://extensions)")
                        .clicked()
                    {
                        self.open_chrome_extensions_page();
                    }
                });

                ui.add_space(6.0);
                ui.label(
                    RichText::new(&self.getube_status)
                        .size(12.5)
                        .color(theme.status_text()),
                );
            },
        );

        ui.add_space(8.0);
        theme.render_card(
            ui,
            Some("Paralel Range Parçaları (--jobs) & Native Messaging Hizalaması"),
            |ui| {
                ui.label(
                    RichText::new(format!(
                        "Stüdyo varsayılanı: --jobs {} paralel parça (aralık: 1..=16). RuviTube Chrome eklentisi (popup/options) seçilen jobs parametresini NativeHost.exe üzerinden doğrudan ruvid motoruna iletir.",
                        self.jobs_count.max(1)
                    ))
                    .color(theme.status_text()),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Varsayılan --jobs Değeri:");
                    ui.add(egui::Slider::new(&mut self.jobs_count, 1..=16).suffix(" parça"));
                });
            },
        );
    }

    pub fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        self.render_page_heading(
            ui,
            ActiveTab::Settings,
            "Ayarlar — Tema, İndirme Varsayılanları & Motor",
        );
        ui.add_space(6.0);

        // Card 1: Theme & Appearance
        theme.render_card(ui, Some("Görünüm & OxiBot / GitHub Tema Modu"), |ui| {
            ui.horizontal(|ui| {
                ui.label("Arayüz Teması:");
                if ui
                    .selectable_label(
                        self.current_theme == StudioTheme::Dark,
                        "Koyu (OxiBot GitHub Dark)",
                    )
                    .clicked()
                {
                    self.current_theme = StudioTheme::Dark;
                    ui.ctx().set_visuals(self.current_theme.visuals());
                }
                if ui
                    .selectable_label(
                        self.current_theme == StudioTheme::Light,
                        "Açık (GitHub Light)",
                    )
                    .clicked()
                {
                    self.current_theme = StudioTheme::Light;
                    ui.ctx().set_visuals(self.current_theme.visuals());
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new("Üst çubuktaki tema düğmesinden de anlık geçiş yapabilirsiniz.")
                    .size(11.5)
                    .color(theme.text_muted()),
            );
        });
        ui.add_space(8.0);

        // Card 2: Download & Parallel Range Defaults
        theme.render_card(
            ui,
            Some("Varsayılan İndirme & Paralel Range (--jobs)"),
            |ui| {
                egui::Grid::new("settings_download_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Varsayılan Çıktı Şablonu:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.output_path_input)
                                .desired_width(420.0)
                                .hint_text("./downloads/%(title)s.%(ext)s"),
                        );
                        ui.end_row();

                        ui.label("Paralel Range Parçaları (--jobs):");
                        ui.add(egui::Slider::new(&mut self.jobs_count, 1..=16).suffix(" parça"));
                        ui.end_row();

                        ui.label("Varsayılan Format:");
                        ui.horizontal_wrapped(|ui| {
                            for preset in ["best", "2160p", "1080p", "720p", "worst"] {
                                ui.selectable_value(
                                    &mut self.format_spec,
                                    preset.to_string(),
                                    preset,
                                );
                            }
                        });
                        ui.end_row();

                        ui.label("Dosya Yazım Davranışı:");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.resume_enabled, "Resume Checkpoint");
                            ui.checkbox(&mut self.overwrite_enabled, "Üzerine Yaz (--overwrite)");
                        });
                        ui.end_row();
                    });
            },
        );
        ui.add_space(8.0);

        // Card 3: Subtitles & Audio Defaults
        theme.render_card(ui, Some("Altyazı & Ses Varsayılanları"), |ui| {
            egui::Grid::new("settings_subtitle_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Altyazı Seçenekleri:");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.download_subtitles, "Altyazıları İndir");
                        ui.checkbox(&mut self.auto_subtitles, "Otomatik (ASR)");
                        for fmt in ["srt", "vtt", "sbv"] {
                            ui.selectable_value(&mut self.subtitle_format, fmt.to_string(), fmt);
                        }
                    });
                    ui.end_row();

                    ui.label("Varsayılan Diller:");
                    ui.horizontal(|ui| {
                        ui.label("Altyazı Dili:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.subtitle_lang)
                                .desired_width(60.0)
                                .hint_text("tr/en"),
                        );
                        ui.label("Ses Dili:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.audio_lang)
                                .desired_width(60.0)
                                .hint_text("tr/en"),
                        );
                    });
                    ui.end_row();
                });
        });
        ui.add_space(8.0);

        // Card 4: RuviTube Directory & Engine Info
        theme.render_card(ui, Some("RuviTube Dizini & Saf Rust Motor Bilgisi"), |ui| {
            ui.horizontal(|ui| {
                ui.label("RuviTube Dizini:");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.getube_dir)
                            .desired_width(340.0)
                            .hint_text("./ruvitube"),
                    )
                    .changed()
                {
                    self.check_getube_status();
                }
                if ui.button("Yeniden Tara").clicked() {
                    self.check_getube_status();
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "RuviStudio v{}  |  ruvid + ruvic v0.1.0  |  Saf Rust (Sıfır FFmpeg / yt-dlp / Deno bağımlılığı)",
                    env!("CARGO_PKG_VERSION")
                ))
                .size(12.0)
                .color(theme.text_muted()),
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
            self.error_banner = Some("Lütfen geçerli bir URL girin.".to_string());
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
            self.error_banner = Some("Lütfen indirilecek URL'yi girin.".to_string());
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
            self.error_banner = Some("Lütfen yoklanacak medya dosyası yolunu girin.".to_string());
            return;
        }
        self.busy = true;
        self.error_banner = None;
        self.status_line = format!("Yoklaniyor: {path_str} ...");
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let start = Instant::now();
            match probe_media_file(Path::new(&path_str)) {
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
            self.error_banner = Some("Lütfen girdi ve çıktı dosya yollarını belirtin.".to_string());
            return;
        }
        let secondary = if self.media_secondary_audio_path.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(self.media_secondary_audio_path.trim()))
        };
        let op = self.media_op;
        let convert_video = if self.convert_video_codec.trim().is_empty() {
            None
        } else {
            Some(self.convert_video_codec.trim().to_string())
        };
        let convert_audio = if self.convert_audio_codec.trim().is_empty() {
            None
        } else {
            Some(self.convert_audio_codec.trim().to_string())
        };
        let convert_preset = if self.convert_preset.trim().is_empty() {
            None
        } else {
            Some(self.convert_preset.trim().to_string())
        };
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
                MediaOperationKind::ConvertVideo => {
                    execute_convert(
                        &backend,
                        &config,
                        None,
                        ConvertRequest {
                            input,
                            output,
                            video: convert_video,
                            audio: convert_audio,
                            quality: None,
                            speed: None,
                            preset: convert_preset,
                            overwrite: true,
                            threads: None,
                            hwaccel: None,
                            video_stream: None,
                            audio_stream: None,
                            subtitle: None,
                        },
                        false,
                    )
                    .await
                }
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
                Some("Lütfen bir Playlist, Podcast RSS veya M3U/JSON adresi girin.".to_string());
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

    fn render_nav_item(&mut self, ui: &mut egui::Ui, tab: ActiveTab) {
        let theme = self.current_theme;
        let is_selected = self.active_tab == tab;
        let desired_size = Vec2::new(ui.available_width(), 44.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        // Fluent-style eased transitions for hover / selection (120ms / 180ms).
        let ctx = ui.ctx().clone();
        let hover_t = ctx.animate_bool_with_time(
            response.id.with("nav_hover"),
            response.hovered() && !is_selected,
            0.12,
        );
        let select_t =
            ctx.animate_bool_with_time(response.id.with("nav_select"), is_selected, 0.18);

        let bg = if select_t > 0.0 {
            theme.nav_item_active_fill().gamma_multiply(select_t)
        } else if hover_t > 0.0 {
            theme.nav_item_hover_fill().gamma_multiply(hover_t)
        } else {
            Color32::TRANSPARENT
        };

        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 6.0, bg);
        }

        if select_t > 0.0 {
            let bar_h = 22.0 * select_t;
            let accent_rect = Rect::from_min_size(
                egui::Pos2::new(rect.left() + 2.0, rect.center().y - bar_h / 2.0),
                Vec2::new(3.0, bar_h),
            );
            ui.painter()
                .rect_filled(accent_rect, 1.5, theme.accent_secondary());
        }

        let icon_color = if is_selected {
            theme.accent_secondary()
        } else {
            theme.text_muted()
        };
        let icon_rect = Rect::from_center_size(
            egui::Pos2::new(rect.left() + 22.0, rect.center().y),
            Vec2::new(18.0, 18.0),
        );
        icons::draw_tab_icon(ui.painter(), icon_rect, tab, icon_color);

        let text_left = rect.left() + 40.0;
        let title_color = if is_selected {
            theme.card_header_text()
        } else {
            theme.status_text()
        };
        ui.painter().text(
            egui::Pos2::new(text_left, rect.center().y - 7.5),
            egui::Align2::LEFT_CENTER,
            tab.label(),
            egui::FontId::proportional(12.5),
            title_color,
        );
        ui.painter().text(
            egui::Pos2::new(text_left, rect.center().y + 7.5),
            egui::Align2::LEFT_CENTER,
            tab.subtitle(),
            egui::FontId::proportional(10.0),
            theme.text_muted(),
        );

        if response.clicked() {
            self.active_tab = tab;
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;

        ui.add_space(4.0);
        ui.label(
            RichText::new("MENÜ")
                .size(10.5)
                .color(theme.text_muted())
                .strong(),
        );
        ui.add_space(6.0);

        for tab in ActiveTab::primary_tabs().iter().copied() {
            self.render_nav_item(ui, tab);
            ui.add_space(3.0);
        }

        // Bottom-up footer: compact engine card + 1px divider + pinned Settings tab
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(4.0);
            let frame = theme.card_frame().inner_margin(10.0);
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let dot_color = if self.error_banner.is_some() {
                        theme.status_error_colors().2
                    } else if self.busy {
                        theme.status_busy_colors().2
                    } else {
                        theme.status_ready_colors().2
                    };
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                    icons::draw_status_dot(ui.painter(), dot_rect.center(), 3.5, dot_color);
                    ui.add_space(4.0);
                    let status_msg = if self.error_banner.is_some() {
                        "Motor: Hata"
                    } else if self.busy {
                        "İşlem Sürüyor..."
                    } else {
                        "Motor: Hazır"
                    };
                    ui.label(
                        RichText::new(status_msg)
                            .size(11.0)
                            .strong()
                            .color(theme.status_text()),
                    );
                });
                ui.add_space(3.0);
                ui.label(
                    RichText::new(format!("RuviStudio v{}", env!("CARGO_PKG_VERSION")))
                        .size(10.5)
                        .color(theme.card_header_text()),
                );
                ui.label(
                    RichText::new("ruvid + ruvic v0.1.0")
                        .size(10.0)
                        .color(theme.text_muted()),
                );
            });

            ui.add_space(6.0);
            let div_rect = ui.allocate_space(Vec2::new(ui.available_width(), 1.0)).1;
            ui.painter()
                .rect_filled(div_rect, 0.0, theme.border_color());
            ui.add_space(6.0);

            self.render_nav_item(ui, ActiveTab::Settings);
        });
    }

    fn render_title_bar(&mut self, ctx: &egui::Context) {
        let theme = self.current_theme;
        let panel_res = egui::TopBottomPanel::top("title_bar")
            .exact_height(48.0)
            .frame(theme.titlebar_frame())
            .show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.horizontal(|ui| {
                        // Left cluster: Brand badge + Title + Version pill + Live status pill
                        let (badge_rect, _) =
                            ui.allocate_exact_size(Vec2::new(24.0, 24.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(badge_rect, 5.0, theme.brand_fill());
                        ui.painter().text(
                            badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "R",
                            egui::FontId::proportional(13.5),
                            Color32::WHITE,
                        );
                        ui.add_space(6.0);

                        ui.label(
                            RichText::new("RuviStudio")
                                .size(14.0)
                                .strong()
                                .color(theme.card_header_text()),
                        );
                        ui.add_space(4.0);

                        // Version pill
                        egui::Frame::NONE
                            .fill(theme.nav_item_active_fill())
                            .stroke(Stroke::new(1.0_f32, theme.border_color()))
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                        .size(10.5)
                                        .strong()
                                        .color(theme.accent_purple()),
                                );
                            });
                        ui.add_space(6.0);

                        // Live Engine Status Pill (Ready green / Busy amber / Error red)
                        let ((status_bg, status_border, status_fg), badge_text) =
                            if self.error_banner.is_some() {
                                (theme.status_error_colors(), "Motor: Hata")
                            } else if self.busy {
                                (theme.status_busy_colors(), "Motor: İşlem Sürüyor")
                            } else {
                                (theme.status_ready_colors(), "Motor: Hazır")
                            };

                        egui::Frame::NONE
                            .fill(status_bg)
                            .stroke(Stroke::new(1.0_f32, status_border))
                            .corner_radius(11.0)
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (dot_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(6.0, 6.0),
                                        egui::Sense::hover(),
                                    );
                                    icons::draw_status_dot(
                                        ui.painter(),
                                        dot_rect.center(),
                                        3.0,
                                        status_fg,
                                    );
                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(badge_text)
                                            .size(10.5)
                                            .strong()
                                            .color(status_fg),
                                    );
                                });
                            });

                        // Right cluster: Theme switcher + Quick Settings icon + Telemetry pill
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let toggle_name = match self.current_theme {
                                StudioTheme::Dark => "Koyu",
                                StudioTheme::Light => "Açık",
                            };
                            let (btn_rect, btn_resp) =
                                ui.allocate_exact_size(Vec2::new(76.0, 30.0), egui::Sense::click());
                            let btn_bg = if btn_resp.hovered() {
                                theme.nav_item_hover_fill()
                            } else {
                                theme.nav_item_active_fill()
                            };
                            ui.painter().rect(
                                btn_rect,
                                6.0,
                                btn_bg,
                                Stroke::new(1.0_f32, theme.border_color()),
                                egui::StrokeKind::Inside,
                            );
                            let icon_rect = Rect::from_center_size(
                                egui::Pos2::new(btn_rect.left() + 16.0, btn_rect.center().y),
                                Vec2::new(14.0, 14.0),
                            );
                            icons::draw_theme_icon(
                                ui.painter(),
                                icon_rect,
                                self.current_theme,
                                theme.accent_secondary(),
                            );
                            ui.painter().text(
                                egui::Pos2::new(btn_rect.left() + 30.0, btn_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                toggle_name,
                                egui::FontId::proportional(11.5),
                                theme.status_text(),
                            );
                            if btn_resp.clicked() {
                                self.current_theme = match self.current_theme {
                                    StudioTheme::Dark => StudioTheme::Light,
                                    StudioTheme::Light => StudioTheme::Dark,
                                };
                                ctx.set_visuals(self.current_theme.visuals());
                            }
                            if btn_resp.hovered() {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            ui.add_space(6.0);

                            // Quick Settings icon button
                            let (gear_btn_rect, gear_resp) =
                                ui.allocate_exact_size(Vec2::new(30.0, 30.0), egui::Sense::click());
                            let gear_bg = if self.active_tab == ActiveTab::Settings {
                                theme.nav_item_active_fill()
                            } else if gear_resp.hovered() {
                                theme.nav_item_hover_fill()
                            } else {
                                Color32::TRANSPARENT
                            };
                            if gear_bg != Color32::TRANSPARENT {
                                ui.painter().rect(
                                    gear_btn_rect,
                                    6.0,
                                    gear_bg,
                                    Stroke::new(1.0_f32, theme.border_color()),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let gear_icon_rect = Rect::from_center_size(
                                gear_btn_rect.center(),
                                Vec2::new(16.0, 16.0),
                            );
                            let gear_color = if self.active_tab == ActiveTab::Settings {
                                theme.accent_secondary()
                            } else {
                                theme.status_text()
                            };
                            icons::draw_tab_icon(
                                ui.painter(),
                                gear_icon_rect,
                                ActiveTab::Settings,
                                gear_color,
                            );
                            if gear_resp.clicked() {
                                self.active_tab = ActiveTab::Settings;
                            }
                            if gear_resp.hovered() {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            gear_resp.on_hover_text("Ayarlar");

                            ui.add_space(6.0);

                            // Telemetry pill
                            egui::Frame::NONE
                                .fill(theme.nav_item_active_fill())
                                .stroke(Stroke::new(1.0_f32, theme.border_color()))
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(10, 4))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Tamamlanan: {}  |  {:.2} MB",
                                            self.telemetry.jobs_completed,
                                            self.telemetry.total_bytes_downloaded as f64
                                                / (1024.0 * 1024.0)
                                        ))
                                        .size(11.0)
                                        .strong()
                                        .color(theme.accent_secondary()),
                                    );
                                });
                        });
                    });
                });
            });

        // Paint crisp 1px bottom border along TitleBar
        let rect = panel_res.response.rect;
        ctx.layer_painter(egui::LayerId::background()).line_segment(
            [
                egui::Pos2::new(rect.left(), rect.bottom()),
                egui::Pos2::new(rect.right(), rect.bottom()),
            ],
            Stroke::new(1.0_f32, theme.border_color()),
        );
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        if !self.theme_initialized {
            ctx.set_visuals(self.current_theme.visuals());
            self.theme_initialized = true;
        }

        self.poll_events();
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }

        let theme = self.current_theme;

        // 1. Top TitleBar (OxiBot.Admin 48px)
        self.render_title_bar(ctx);

        // 2. Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_status")
            .exact_height(28.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_fill())
                    .stroke(Stroke::new(1.0_f32, theme.border_color()))
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(&self.status_line)
                            .size(11.5)
                            .color(theme.status_text()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("Saf Rust · Sıfır FFmpeg / yt-dlp Bağımlılığı")
                                .size(11.0)
                                .color(theme.text_muted()),
                        );
                    });
                });
            });

        // 3. Left Sidebar (OxiBot.Admin PaneDisplayMode="Left", 240px)
        let sidebar_res = egui::SidePanel::left("left_sidebar")
            .resizable(false)
            .exact_width(240.0)
            .frame(theme.sidebar_frame())
            .show(ctx, |ui| {
                self.render_sidebar(ui);
            });
        let s_rect = sidebar_res.response.rect;
        ctx.layer_painter(egui::LayerId::background()).line_segment(
            [
                egui::Pos2::new(s_rect.right(), s_rect.top()),
                egui::Pos2::new(s_rect.right(), s_rect.bottom()),
            ],
            Stroke::new(1.0_f32, theme.border_color()),
        );

        // 4. Central Content Area
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme.visuals().panel_fill)
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                if let Some(err) = self.error_banner.clone() {
                    egui::Frame::NONE
                        .fill(theme.error_banner_fill())
                        .stroke(Stroke::new(1.0_f32, theme.error_banner_stroke()))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .corner_radius(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (warn_rect, _) = ui.allocate_exact_size(
                                    Vec2::new(16.0, 16.0),
                                    egui::Sense::hover(),
                                );
                                icons::draw_warning_icon(
                                    ui.painter(),
                                    warn_rect,
                                    theme.error_banner_text(),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(err).strong().color(theme.error_banner_text()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("Kapat").clicked() {
                                            self.error_banner = None;
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(10.0);
                }

                ScrollArea::vertical().show(ui, |ui| match self.active_tab {
                    ActiveTab::Downloader => self.render_downloader_tab(ui),
                    ActiveTab::MediaStudio => self.render_media_studio_tab(ui),
                    ActiveTab::PlaylistQueue => self.render_playlist_tab(ui),
                    ActiveTab::Telemetry => self.render_telemetry_tab(ui),
                    ActiveTab::RuviTube => self.render_ruvitube_tab(ui),
                    ActiveTab::Settings => self.render_settings_tab(ui),
                });
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
            self.error_banner = Some("İndirilecek en az bir playlist öğesi seçin.".to_string());
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Re-apply native Win11 chrome (dark title bar, rounded corners, Mica) on theme change.
        if self.chrome_applied_theme != Some(self.current_theme) {
            crate::win11::apply_window_chrome(frame, self.current_theme == StudioTheme::Dark);
            self.chrome_applied_theme = Some(self.current_theme);
        }
        self.render_ui(ctx);
    }
}

impl RuviStudioApp {
    fn render_downloader_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme;
        self.render_page_heading(
            ui,
            ActiveTab::Downloader,
            "YouTube / DASH 4K-8K / HLS / HTTP İndirici",
        );
        ui.add_space(6.0);

        // Card 1: URL & Stream Resolution
        theme.render_card(ui, Some("Hedef Medya URL'si & Akış Çözümleme"), |ui| {
            ui.horizontal(|ui| {
                ui.label("Medya / YouTube URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.url_input)
                        .desired_width(440.0)
                        .hint_text("https://www.youtube.com/watch?v=..."),
                );
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Akışları Çözümle (Info)"))
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
        theme.render_card(
            ui,
            Some("Format, Kalite & Paralel İndirme Seçenekleri"),
            |ui| {
                egui::Grid::new("dl_format_opts_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Çıktı Şablonu / Dosya:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.output_path_input)
                                .desired_width(440.0)
                                .hint_text("./downloads/%(title)s.%(ext)s"),
                        );
                        ui.end_row();

                        ui.label("Format / Çözünürlük:");
                        ui.horizontal_wrapped(|ui| {
                            for preset in [
                                "best", "4320p", "2160p", "1080p", "720p", "360p", "240p", "worst",
                            ] {
                                ui.selectable_value(
                                    &mut self.format_spec,
                                    preset.to_string(),
                                    preset,
                                );
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
                            ui.add(
                                egui::Slider::new(&mut self.jobs_count, 1..=16).suffix(" parça"),
                            );
                            ui.checkbox(&mut self.resume_enabled, "Resume Checkpoint");
                            ui.checkbox(&mut self.overwrite_enabled, "Üzerine Yaz (--overwrite)");
                        });
                        ui.end_row();
                    });
            },
        );
        ui.add_space(8.0);

        // Card 3: Subtitles, Audio & Clip Settings
        theme.render_card(ui, Some("Altyazı, Dublaj Ses & Klip Parametreleri"), |ui| {
            egui::Grid::new("dl_sub_clip_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Altyazı & Dublaj Ses:");
                    ui.horizontal_wrapped(|ui| {
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
                    ui.horizontal_wrapped(|ui| {
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
                        RichText::new("Paralel İndirmeyi Başlat (ruvid + Otomatik DASH Remux)")
                            .size(14.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(theme.brand_fill())
                    .min_size(Vec2::new(380.0, 36.0)),
                )
                .clicked()
            {
                self.spawn_download();
            }
        });

        // Dynamic Outcome Card
        if let Some(outcome) = self.last_download.clone() {
            ui.add_space(10.0);
            theme.render_card(ui, Some("Son İndirme Özeti"), |ui| {
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
                        "İndirilen Altyazılar ({}): {}",
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
                        "Bölünen Chapter Dosyaları ({}): {}",
                        outcome.chapters.len(),
                        outcome.chapters.join(", ")
                    ));
                }
            });
        }

        // Dynamic Streams Card
        if let Some(source) = self.resolved_source.clone() {
            ui.add_space(10.0);
            let title_str = format!("Çözülen Kaynak: {}", source.title);
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
        self.render_page_heading(
            ui,
            ActiveTab::MediaStudio,
            "ruvic Saf Rust Native Medya Motoru (Sıfır FFmpeg Bağımlılığı)",
        );
        ui.label(
            RichText::new(
                "Seek tabanlı 64KB/1MB Streaming I/O · MP4 stco/co64 Offset Rebasing · ADTS AAC & M4A Demux · 360/VR & HDR Koruma",
            )
            .color(theme.text_muted()),
        );
        ui.add_space(8.0);

        // Card 1: Input & Probe
        theme.render_card(ui, Some("Girdi Medyası & Akış Yoklama (Probe)"), |ui| {
            egui::Grid::new("media_studio_input_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Girdi Medya Dosyası:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.media_input_path)
                                .desired_width(420.0)
                                .hint_text("./downloads/video.mp4"),
                        );
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Yokla (Probe)"))
                            .clicked()
                        {
                            self.spawn_probe_media();
                        }
                    });
                    ui.end_row();

                    ui.label("İkincil Ses Dosyası (DASH Mux):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.media_secondary_audio_path)
                            .desired_width(420.0)
                            .hint_text("Boş bırakılabilir veya ayrık ses dosyası (.m4a / .webm)"),
                    );
                    ui.end_row();
                });
        });
        ui.add_space(8.0);

        // Card 2: Operation & Parameters
        theme.render_card(
            ui,
            Some("ruvic Native İşlem Seçimi & Parametreler"),
            |ui| {
                egui::Grid::new("media_studio_op_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("İşlem Türü:");
                        ui.horizontal_wrapped(|ui| {
                            for kind in [
                                MediaOperationKind::ConvertVideo,
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

                        if self.media_op == MediaOperationKind::ConvertVideo {
                            ui.label("Video Codec:");
                            ui.horizontal(|ui| {
                                for v in ["h264", "h265", "vp9", "av1", "copy"] {
                                    if ui
                                        .selectable_label(self.convert_video_codec == v, v)
                                        .clicked()
                                    {
                                        self.convert_video_codec = v.to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Ses Codec:");
                            ui.horizontal(|ui| {
                                for a in ["aac", "opus", "mp3", "flac", "copy"] {
                                    if ui
                                        .selectable_label(self.convert_audio_codec == a, a)
                                        .clicked()
                                    {
                                        self.convert_audio_codec = a.to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Hız Profili (Preset):");
                            ui.horizontal(|ui| {
                                for p in ["ultrafast", "fast", "medium", "slow", "veryslow"] {
                                    if ui.selectable_label(self.convert_preset == p, p).clicked() {
                                        self.convert_preset = p.to_string();
                                    }
                                }
                            });
                            ui.end_row();
                        } else if self.media_op == MediaOperationKind::TrimRange {
                            ui.label("Trim Başlangıç & Süre (sn):");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.trim_start)
                                        .desired_width(90.0),
                                );
                                ui.add(
                                    egui::Slider::new(&mut self.trim_duration, 0.5..=600.0)
                                        .suffix(" sn"),
                                );
                            });
                            ui.end_row();
                        } else if self.media_op == MediaOperationKind::Thumbnail {
                            ui.label("Küçük Resim Zamanı (--at):");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.thumbnail_at)
                                    .desired_width(90.0),
                            );
                            ui.end_row();
                        }

                        ui.label("Çıktı Dosya Yolu:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.media_output_path)
                                .desired_width(420.0)
                                .hint_text("./downloads/output.mp4"),
                        );
                        ui.end_row();
                    });
            },
        );
        ui.add_space(10.0);

        // Action button
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(
                    RichText::new("Saf Rust Native Motor İle İşle")
                        .size(14.5)
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(theme.brand_fill())
                .min_size(Vec2::new(320.0, 36.0)),
            )
            .clicked()
        {
            self.spawn_media_operation();
        }

        // Dynamic Result Card
        if let Some(res) = self.last_media_op.clone() {
            ui.add_space(10.0);
            theme.render_card(ui, Some("İşlem Tamamlandı"), |ui| {
                ui.label(
                    RichText::new(format!(
                        "Çıktı: {} — {} bayt ({} ms)",
                        res.output, res.output_size_bytes, res.duration_ms
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
                "Konteyner Analizi: {} ({})",
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
                    let hdr_badge = if s.color_primaries.is_some() || s.mastering_display.is_some()
                    {
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
        self.render_page_heading(
            ui,
            ActiveTab::PlaylistQueue,
            "Toplu Playlist, Dizi ve Podcast İndirici",
        );
        ui.add_space(6.0);

        // Card 1: Playlist & RSS Config
        theme.render_card(ui, Some("Playlist / RSS / M3U Kaynak Ayarları"), |ui| {
            ui.horizontal(|ui| {
                ui.label("Playlist / RSS / M3U URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.playlist_url)
                        .desired_width(420.0)
                        .hint_text("https://www.youtube.com/playlist?list=..."),
                );
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Listeyi Çözümle"))
                    .clicked()
                {
                    self.spawn_resolve_playlist();
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("Çıktı Klasörü:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.playlist_output_dir).desired_width(240.0),
                );
                ui.add(
                    egui::Slider::new(&mut self.playlist_concurrency, 1..=8)
                        .text("Eşzamanlı İndirme"),
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
            theme.render_card(ui, Some("Kuyruktaki İçerikler & Toplu İndirme"), |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(
                                RichText::new("Seçilen Öğeleri İndir")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(theme.success_cta_fill())
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
        self.render_page_heading(
            ui,
            ActiveTab::Telemetry,
            "Performans, Bellek Yükü ve Piyasa Kıyaslama Panosu",
        );
        ui.add_space(6.0);

        ui.columns(3, |cols| {
            theme.render_card(
                &mut cols[0],
                Some("UYGULAMA BOYUTU (SIFIR BAĞIMLILIK)"),
                |ui| {
                    ui.heading(
                        RichText::new("ruvid: 6.87 MB | ruvic: 3.73 MB")
                            .color(theme.accent_primary()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Piyasa Yığını (yt-dlp + deno + ffmpeg): ~360 MB (~34x daha küçük)",
                        )
                        .color(theme.text_muted()),
                    );
                },
            );

            theme.render_card(&mut cols[1], Some("50.3 MB MP4 STREAMING BELLEK"), |ui| {
                ui.heading(
                    RichText::new("3.53 MB Özel RAM (42.36 ms)").color(theme.accent_secondary()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Aktarım Hızı: ~1,187 MB/sn (PeakWorkingSet: 8.98 MB — %7)")
                        .color(theme.text_muted()),
                );
            });

            theme.render_card(
                &mut cols[2],
                Some("SİSTEM BÜTÜNLÜĞÜ & TESTLER"),
                |ui| {
                    ui.heading(
                        RichText::new("209+ Test Yeşil | Exit 8 Koruma")
                            .color(theme.accent_purple()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Atomik .part/.ruvic-tmp yazım + validate_moov_integrity")
                            .color(theme.text_muted()),
                    );
                },
            );
        });

        ui.add_space(12.0);
        if self.telemetry.history.is_empty() {
            theme.render_card(ui, Some("Gerçek Zamanlı Oturum İşlem Geçmişi"), |ui| {
                ui.label(
                    RichText::new(
                        "Henüz bu oturumda bir indirme veya medya işlemi çalıştırılmadı.",
                    )
                    .color(theme.text_muted()),
                );
            });
        } else {
            let mut clear_history = false;
            let history_items = self.telemetry.history.clone();
            theme.render_card(ui, Some("Gerçek Zamanlı Oturum İşlem Geçmişi"), |ui| {
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
