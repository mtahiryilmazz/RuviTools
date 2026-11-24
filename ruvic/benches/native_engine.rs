//! Micro-benchmarks for the pure-Rust native engine.
//!
//! `BENCHMARK_REPORT.md` measures *end-to-end* CLI wall time (process start, tokio
//! runtime, config load, output open). Those numbers cannot tell you whether
//! `ruvic probe` is slower than `ffprobe` because of the parser loop or because of
//! the process/boot cost, and the same is true for `trim`. These benches isolate
//! the engine itself:
//!
//! ```text
//! cargo bench --bench native_engine
//! cargo bench --bench native_engine -- probe      # single group
//! ```
//!
//! Every group works on files/memory built in-process, so the numbers are
//! comparable across machines and stable between runs.

use std::io::Cursor;

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

use ruvic::backend::native::container_probe::probe_media_file;
use ruvic::backend::native::dual_mux::try_trim_fmp4;
use ruvic::backend::native::mp4_atoms::{Mp4BuildSpec, build_mp4_bytes, rewrite_mp4_faststart};
use ruvic::backend::native::timescale::{Round, nanos_to_ticks, ratio_cmp, rescale_u64};
use ruvic::domain::stream::{AudioCodec, ContainerFormat, VideoCodec};

/// 8 MiB of payload — large enough that the copy path dominates the container
/// bookkeeping, small enough that a bench run stays in cache.
const PAYLOAD_LEN: usize = 8 * 1024 * 1024;

fn synthetic_mp4(duration_seconds: f64, payload: &[u8]) -> Vec<u8> {
    build_mp4_bytes(&Mp4BuildSpec {
        container: ContainerFormat::Mp4,
        duration_seconds,
        video: Some((VideoCodec::H264, 1920, 1080)),
        audio: Some((AudioCodec::Aac, 48_000, 2)),
        mdat_payload: payload,
        video_aux: None,
    })
}

fn bench_probe(c: &mut Criterion) {
    let payload = vec![0x5Au8; PAYLOAD_LEN];
    let media = synthetic_mp4(600.0, &payload);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bench_10min.mp4");
    std::fs::write(&path, &media).expect("write fixture");

    let mut group = c.benchmark_group("probe");
    group.throughput(Throughput::Bytes(media.len() as u64));
    group.bench_function("10min_1080p_file", |b| {
        b.iter(|| {
            let probed = probe_media_file(black_box(&path)).expect("probe");
            black_box(probed.info.streams.len())
        });
    });
    group.finish();
}

fn bench_remux(c: &mut Criterion) {
    let payload = vec![0x5Au8; PAYLOAD_LEN];
    let media = synthetic_mp4(600.0, &payload);
    let mut sink = Vec::with_capacity(media.len() + 64 * 1024);

    let mut group = c.benchmark_group("remux_faststart");
    group.throughput(Throughput::Bytes(media.len() as u64));
    group.bench_function("8MiB_moov_reorder", |b| {
        b.iter(|| {
            sink.clear();
            let mut reader = Cursor::new(black_box(&media));
            rewrite_mp4_faststart(&mut reader, &mut sink, media.len() as u64).expect("remux");
            black_box(sink.len())
        });
    });
    group.finish();
}

fn bench_trim(c: &mut Criterion) {
    let payload = vec![0x5Au8; PAYLOAD_LEN];
    let media = synthetic_mp4(600.0, &payload);
    let mut sink = Vec::with_capacity(media.len() / 8);

    let mut group = c.benchmark_group("trim");
    group.bench_function("mp4_5s_of_10min", |b| {
        b.iter(|| {
            sink.clear();
            let mut reader = Cursor::new(black_box(&media));
            let ok = try_trim_fmp4(&mut reader, &mut sink, 12.0, 5.0).expect("trim");
            black_box(ok)
        });
    });
    group.finish();
}

/// The new integer timescale path: proves the overflow-safe rescale costs nothing
/// measurable next to the `f64` arithmetic it replaced.
fn bench_timescale(c: &mut Criterion) {
    let mut group = c.benchmark_group("timescale");
    group.bench_function("rescale_u64_10h_48khz", |b| {
        let ticks = 10u64 * 3600 * 48_000;
        b.iter(|| black_box(rescale_u64(black_box(ticks), 1000, 48_000, Round::Nearest)));
    });
    group.bench_function("nanos_to_ticks_ntsc", |b| {
        b.iter(|| {
            black_box(nanos_to_ticks(
                black_box(36_000_000_000_000),
                90_000,
                Round::Nearest,
            ))
        });
    });
    group.bench_function("ratio_cmp_mixed_timebases", |b| {
        b.iter(|| black_box(ratio_cmp(black_box(1_234_567), 90_000, 987_654, 48_000)));
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_probe,
    bench_remux,
    bench_trim,
    bench_timescale
);
criterion_main!(benches);
