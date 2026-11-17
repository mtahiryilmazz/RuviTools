use assert_cmd::Command;
use eframe::egui::{self, Color32};
use predicates::prelude::*;
use ruvi_gui::{ActiveTab, MediaOperationKind, RuviStudioApp, StudioTheme, configure_fonts, icons};

#[test]
fn app_builds_download_job_with_all_options() {
    let mut app = RuviStudioApp::default();
    assert_eq!(app.active_tab, ActiveTab::Downloader);
    assert_eq!(app.media_op, MediaOperationKind::RemuxFaststart);

    app.url_input = "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_string();
    app.format_spec = "2160p".to_string();
    app.jobs_count = 8;
    app.download_subtitles = true;
    app.auto_subtitles = true;
    app.subtitle_lang = "tr".to_string();
    app.subtitle_format = "srt".to_string();
    app.audio_lang = "tr".to_string();
    app.split_chapters = true;
    app.clip_start = "00:00:05".to_string();
    app.clip_end = "00:00:20".to_string();
    app.clip_duration = "15".to_string();

    let job = app.build_download_job();
    assert_eq!(job.url, "https://www.youtube.com/watch?v=jNQXAC9IVRw");
    assert_eq!(job.format_spec, "2160p");
    assert_eq!(job.jobs, 8);
    assert!(job.subtitles);
    assert!(job.auto_subs);
    assert_eq!(job.subtitles_lang.as_deref(), Some("tr"));
    assert_eq!(job.sub_format, "srt");
    assert_eq!(job.audio_lang.as_deref(), Some("tr"));
    assert!(job.split_chapters);
    assert_eq!(job.start_time.as_deref(), Some("00:00:05"));
    assert_eq!(job.end_time.as_deref(), Some("00:00:20"));
    assert_eq!(job.duration_seconds, Some(15.0));
}

#[test]
fn all_six_tabs_metadata_and_navigation() {
    let primary = ActiveTab::primary_tabs();
    assert_eq!(primary.len(), 5);
    assert!(!primary.contains(&ActiveTab::Settings));

    let all = ActiveTab::all();
    assert_eq!(
        all,
        &[
            ActiveTab::Downloader,
            ActiveTab::MediaStudio,
            ActiveTab::PlaylistQueue,
            ActiveTab::Telemetry,
            ActiveTab::RuviTube,
            ActiveTab::Settings,
        ]
    );

    for &tab in all {
        assert!(!tab.icon().trim().is_empty());
        assert!(!tab.label().trim().is_empty());
        assert_eq!(tab.title(), tab.label());
        assert!(!tab.subtitle().trim().is_empty());
    }
}

#[test]
fn oxibot_dark_and_light_theme_tokens() {
    assert_eq!(StudioTheme::default(), StudioTheme::Dark);

    let dark = StudioTheme::Dark;
    let light = StudioTheme::Light;

    let dark_vis = dark.visuals();
    let light_vis = light.visuals();
    assert!(dark_vis.dark_mode);
    assert!(!light_vis.dark_mode);

    // Verify exact OxiBot GitHub Dark & GitHub Light tokens
    assert_eq!(dark_vis.panel_fill, Color32::from_rgb(13, 17, 23));
    assert_eq!(dark_vis.window_fill, Color32::from_rgb(22, 27, 34));
    assert_eq!(dark.border_color(), Color32::from_rgb(48, 54, 61));
    assert_eq!(dark.brand_fill(), Color32::from_rgb(31, 111, 235));
    assert_eq!(dark.accent_primary(), Color32::from_rgb(63, 185, 80));
    assert_eq!(dark.accent_secondary(), Color32::from_rgb(88, 166, 255));
    assert_eq!(dark.accent_purple(), Color32::from_rgb(168, 85, 247));

    assert_eq!(light_vis.panel_fill, Color32::from_rgb(246, 248, 250));
    assert_eq!(light_vis.window_fill, Color32::from_rgb(255, 255, 255));
    assert_eq!(light.border_color(), Color32::from_rgb(208, 215, 222));
    assert_eq!(light.brand_fill(), Color32::from_rgb(9, 105, 218));

    assert_ne!(dark.accent_primary(), light.accent_primary());
    assert_ne!(dark.accent_secondary(), light.accent_secondary());
    assert_ne!(dark.accent_purple(), light.accent_purple());
    assert_ne!(dark.text_muted(), light.text_muted());
    assert_ne!(dark.status_text(), light.status_text());
    assert_ne!(dark.error_banner_fill(), light.error_banner_fill());
    assert_ne!(dark.error_banner_stroke(), light.error_banner_stroke());
    assert_ne!(dark.error_banner_text(), light.error_banner_text());
    assert_ne!(dark.nav_item_active_fill(), light.nav_item_active_fill());
    assert_ne!(dark.nav_item_hover_fill(), light.nav_item_hover_fill());

    let dark_frame = dark.card_frame();
    let light_frame = light.card_frame();
    assert_ne!(dark_frame.fill, light_frame.fill);
}

#[test]
fn headless_egui_renders_all_tabs_and_themes_without_panic() {
    let mut app = RuviStudioApp::default();
    let ctx = egui::Context::default();
    configure_fonts(&ctx);

    for theme in [StudioTheme::Dark, StudioTheme::Light] {
        app.set_theme(theme);
        for (idx, &tab) in ActiveTab::all().iter().enumerate() {
            app.active_tab = tab;
            app.busy = idx % 2 == 1;
            app.error_banner = if idx == 0 {
                Some("Test InfoBar warning banner".to_string())
            } else {
                None
            };

            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(1200.0, 760.0),
                )),
                ..Default::default()
            };
            let output = ctx.run(raw_input, |ctx| {
                app.render_ui(ctx);
            });
            assert!(
                !output.shapes.is_empty(),
                "expected rendered egui shapes for tab {tab:?} in theme {theme:?}"
            );
        }
    }
}

#[test]
fn painter_vector_icons_draw_cleanly_for_all_tabs_and_states() {
    let ctx = egui::Context::default();
    let mut painted_shapes = 0usize;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(18.0));
            for &tab in ActiveTab::all() {
                icons::draw_tab_icon(ui.painter(), rect, tab, Color32::WHITE);
            }
            icons::draw_theme_icon(ui.painter(), rect, StudioTheme::Dark, Color32::WHITE);
            icons::draw_theme_icon(ui.painter(), rect, StudioTheme::Light, Color32::YELLOW);
            icons::draw_warning_icon(ui.painter(), rect, Color32::RED);
            icons::draw_check_icon(ui.painter(), rect, Color32::GREEN);
            icons::draw_cross_icon(ui.painter(), rect, Color32::RED);
            icons::draw_status_dot(ui.painter(), rect.center(), 3.0, Color32::GREEN);
            painted_shapes += 1;
        });
    });
    assert_eq!(painted_shapes, 1);
}

#[test]
fn app_theme_toggle_and_ruvitube_detection_and_jobs_state() {
    let mut app = RuviStudioApp::default();
    assert_eq!(app.current_theme, StudioTheme::Dark);
    assert_eq!(app.theme(), StudioTheme::Dark);

    app.set_theme(StudioTheme::Light);
    assert_eq!(app.current_theme, StudioTheme::Light);
    assert_eq!(app.theme(), StudioTheme::Light);

    app.jobs_count = 12;
    assert_eq!(app.build_download_job().jobs, 12);

    assert!(!app.getube_dir.trim().is_empty());
    assert!(
        app.getube_dir.contains("ruvitube") || std::path::Path::new(&app.getube_dir).exists(),
        "expected RuviTube directory to auto-detect ./ruvitube or ../ruvitube, got {}",
        app.getube_dir
    );
    app.check_getube_status();
    assert!(!app.getube_status.is_empty());

    // RuviTube compatibility aliases
    app.check_ruvitube_status();
    assert!(!app.getube_status.is_empty());
}

#[test]
fn cli_version_and_headless_self_test_succeed() {
    Command::cargo_bin("ruvi-studio")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ruvi-studio 0.1.0"));

    Command::cargo_bin("ruvi-studio")
        .unwrap()
        .arg("--headless-self-test")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "ruvi-studio headless self-test: OK",
        ));
}
