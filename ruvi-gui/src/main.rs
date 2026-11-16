use std::sync::Arc;

use eframe::egui;
use ruvi_gui::RuviStudioApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ruvi-studio {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "RuviStudio {} — Pure-Rust Desktop Studio for ruvid & ruvic\nUsage: ruvi-studio [--version] [--help] [--headless-self-test]",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--headless-self-test") {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime"),
        );
        let mut app = RuviStudioApp::new(rt);
        let job = app.build_download_job();
        assert_eq!(job.jobs, 4);
        app.poll_events();
        println!("ruvi-studio headless self-test: OK");
        return Ok(());
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([860.0, 560.0])
            .with_title("RuviStudio — Saf Rust Video İndirici (ruvid) & Medya Motoru (ruvic)"),
        ..Default::default()
    };

    eframe::run_native(
        "RuviStudio",
        native_options,
        Box::new(|_cc| Ok(Box::new(RuviStudioApp::default()))),
    )
}
