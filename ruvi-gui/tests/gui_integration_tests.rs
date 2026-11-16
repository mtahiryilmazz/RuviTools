use assert_cmd::Command;
use predicates::prelude::*;
use ruvi_gui::{ActiveTab, MediaOperationKind, RuviStudioApp, StudioTheme};

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
fn theme_switching_visuals_and_card_frames() {
    assert_eq!(StudioTheme::default(), StudioTheme::Dark);

    let dark = StudioTheme::Dark;
    let light = StudioTheme::Light;

    let dark_vis = dark.visuals();
    let light_vis = light.visuals();
    assert!(dark_vis.dark_mode);
    assert!(!light_vis.dark_mode);

    assert_ne!(dark.accent_primary(), light.accent_primary());
    assert_ne!(dark.accent_secondary(), light.accent_secondary());
    assert_ne!(dark.accent_purple(), light.accent_purple());
    assert_ne!(dark.text_muted(), light.text_muted());
    assert_ne!(dark.status_text(), light.status_text());
    assert_ne!(dark.error_banner_fill(), light.error_banner_fill());
    assert_ne!(dark.error_banner_stroke(), light.error_banner_stroke());
    assert_ne!(dark.error_banner_text(), light.error_banner_text());

    let dark_frame = dark.card_frame();
    let light_frame = light.card_frame();
    assert_ne!(dark_frame.fill, light_frame.fill);
}

#[test]
fn app_theme_toggle_and_ruvitube_setup_card() {
    let mut app = RuviStudioApp::default();
    assert_eq!(app.current_theme, StudioTheme::Dark);
    assert_eq!(app.theme(), StudioTheme::Dark);

    app.set_theme(StudioTheme::Light);
    assert_eq!(app.current_theme, StudioTheme::Light);
    assert_eq!(app.theme(), StudioTheme::Light);

    assert!(!app.getube_dir.trim().is_empty());
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
