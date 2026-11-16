use std::io::Write;

use serde::Serialize;

use crate::domain::conversion_job::OperationResult;
use crate::domain::media_info::MediaInfo;
use crate::domain::preset::Preset;
use crate::operations::batch::BatchReport;

pub fn emit_json<T: Serialize>(value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
    }
}

pub fn render_probe_human(info: &MediaInfo, show_streams: bool, show_chapters: bool) {
    println!("File: {}", info.path);
    println!(
        "Format: {} ({})",
        info.container.format_name,
        info.container
            .format_long_name
            .as_deref()
            .unwrap_or("unknown")
    );
    if let Some(dur) = info.container.duration_seconds {
        println!("Duration: {dur:.2}s");
    }
    if let Some(size) = info.container.size_bytes {
        println!("Size: {size} bytes");
    }
    if let Some(br) = info.container.bit_rate {
        println!("Bitrate: {} kbps", br / 1000);
    }

    if show_streams || !show_chapters {
        println!("Streams ({}):", info.streams.len());
        for s in &info.streams {
            let dims = match (s.width, s.height) {
                (Some(w), Some(h)) => format!(" {w}x{h}"),
                _ => String::new(),
            };
            let fps = s.fps.map(|f| format!(" @ {f:.2}fps")).unwrap_or_default();
            let audio_meta = match (s.sample_rate, s.channels) {
                (Some(sr), Some(ch)) => format!(" {sr}Hz {ch}ch"),
                _ => String::new(),
            };
            let lang = s
                .language
                .as_deref()
                .map(|l| format!(" [{l}]"))
                .unwrap_or_default();
            println!(
                "  #{}: {:?} {}{}{}{}{}",
                s.index, s.kind, s.codec_name, dims, fps, audio_meta, lang
            );
        }
    }

    if show_chapters && !info.chapters.is_empty() {
        println!("Chapters ({}):", info.chapters.len());
        for c in &info.chapters {
            println!(
                "  #{}: {:.2}s - {:.2}s {}",
                c.id,
                c.start_seconds,
                c.end_seconds,
                c.title.as_deref().unwrap_or("")
            );
        }
    }
}

pub fn render_operation_human(res: &OperationResult, quiet: bool) {
    if quiet {
        return;
    }
    eprintln!(
        "Completed {:?} -> {} ({} bytes, {} ms)",
        res.operation, res.output, res.output_size_bytes, res.duration_ms
    );
    if let Some(ref reason) = res.hwaccel_fallback_reason {
        eprintln!("Note: {reason}");
    }
}

pub fn render_batch_human(report: &BatchReport, quiet: bool) {
    if quiet {
        return;
    }
    eprintln!(
        "Batch complete: total={}, succeeded={}, failed={}",
        report.total, report.succeeded, report.failed
    );
    for err in &report.errors {
        eprintln!("  Failed {}: {}", err.input, err.error);
    }
}

pub fn render_preset_list_human(presets: &[Preset]) {
    for p in presets {
        println!("{:<14} {}", p.name, p.description);
    }
}

pub fn render_preset_detail_human(p: &Preset) {
    println!("Name: {}", p.name);
    println!("Description: {}", p.description);
    println!(
        "Container: {} (faststart={})",
        p.container.format, p.container.faststart
    );
    if let Some(ref v) = p.video {
        println!(
            "Video: codec={}, quality={:?}, speed={:?}, pix_fmt={:?}",
            v.codec, v.quality, v.preset, v.pixel_format
        );
    }
    if let Some(ref a) = p.audio {
        println!("Audio: codec={}, bitrate={:?}", a.codec, a.bitrate);
    }
}
