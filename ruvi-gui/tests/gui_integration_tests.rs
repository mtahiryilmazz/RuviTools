use assert_cmd::Command;
use predicates::prelude::*;
use ruvi_gui::{ActiveTab, MediaOperationKind, RuviStudioApp};

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
