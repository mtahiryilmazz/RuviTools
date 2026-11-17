use std::sync::Arc;

use eframe::egui;
use ruvi_gui::{RuviStudioApp, configure_fonts};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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

        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.render_headless(ctx);
        });

        println!("ruvi-studio headless self-test: OK");
        return Ok(());
    }

    let icon_data = eframe::icon_data::from_png_bytes(include_bytes!("../assets/ruvi-studio.png"))
        .ok()
        .map(Arc::new);

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1160.0, 740.0])
        .with_min_inner_size([900.0, 580.0])
        .with_title("RuviStudio — Saf Rust Video İndirici (ruvid) & Medya Motoru (ruvic)");

    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "RuviStudio",
        native_options,
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(RuviStudioApp::default()))
        }),
    )
}
