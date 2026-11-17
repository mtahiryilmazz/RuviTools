use std::io::Write;

use serde::Serialize;

use crate::domain::download_job::{DownloadOutcome, PlaylistReport, SimulateReport};
use crate::domain::media_source::MediaSource;
use crate::domain::stream::StreamPayload;

pub fn emit_json<T: Serialize>(value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
    }
}

pub fn render_source_info_human(source: &MediaSource, all_formats: bool) {
    println!("Title: {}", source.title);
    println!("Kind: {:?}", source.kind);
    println!("Streams ({}):", source.streams.len());
    for s in &source.streams {
        let dims = match (s.width, s.height) {
            (Some(w), Some(h)) => format!(" {w}x{h}"),
            _ => String::new(),
        };
        let bw = s
            .bandwidth_bps
            .map(|b| format!(" {} kbps", b / 1000))
            .unwrap_or_default();
        let codecs = s
            .codecs
            .as_deref()
            .map(|c| format!(" [{c}]"))
            .unwrap_or_default();
        let size = match &s.payload {
            StreamPayload::DirectHttp {
                total_bytes: Some(b),
                accept_ranges,
                ..
            } => format!(" {b} bytes (range={accept_ranges})"),
            StreamPayload::Hls { segments, .. } | StreamPayload::Dash { segments, .. }
                if all_formats && !segments.is_empty() =>
            {
                format!(" {} segments", segments.len())
            }
            _ => String::new(),
        };
        println!(
            "  {:<12} {:?} {}{}{}{}{}",
            s.id, s.kind, s.container, dims, bw, codecs, size
        );
    }
    if !source.subtitles.is_empty() {
        println!("Subtitles ({}):", source.subtitles.len());
        for sub in &source.subtitles {
            let tag = if sub.is_auto { " [auto]" } else { "" };
            println!("  {} ({}){tag}", sub.language, sub.format);
        }
    }
    if !source.chapters.is_empty() {
        println!("Chapters ({}):", source.chapters.len());
        for ch in &source.chapters {
            println!(
                "  {:.1}s - {:.1}s: {}",
                ch.start_seconds, ch.end_seconds, ch.title
            );
        }
    }
}

pub fn render_download_human(outcome: &DownloadOutcome, quiet: bool) {
    if quiet {
        return;
    }
    eprintln!(
        "Downloaded {} ({} bytes) -> {}",
        outcome.title, outcome.bytes_downloaded, outcome.output_path
    );
    if let Some(ref m) = outcome.manifest_path {
        eprintln!("Wrote RUVI manifest -> {m}");
    }
}

pub fn render_playlist_human(report: &PlaylistReport, quiet: bool) {
    if quiet {
        return;
    }
    eprintln!(
        "Playlist finished: selected={}, succeeded={}, failed={}",
        report.total_selected, report.succeeded, report.failed
    );
    for err in &report.errors {
        eprintln!("  Item #{} failed: {}", err.index, err.error);
    }
}

pub fn render_simulate_human(report: &SimulateReport, quiet: bool) {
    if quiet {
        return;
    }
    let size_part = report
        .expected_bytes
        .map(|b| format!("{b} bytes"))
        .unwrap_or_else(|| "unknown size".to_string());
    let dims_part = match (report.width, report.height) {
        (Some(w), Some(h)) => format!(" {w}x{h}"),
        _ => String::new(),
    };
    println!(
        "[simulate] {} | stream={} ({}{} | {}) -> {}",
        report.title,
        report.stream_id,
        report.container,
        dims_part,
        size_part,
        report.output_path
    );
}
