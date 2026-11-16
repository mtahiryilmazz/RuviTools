use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::assembly::{write_dual_stream_handoff_manifest, write_handoff_manifest};
use crate::config::RuvidConfig;
use crate::domain::download_job::{DownloadJob, DownloadOutcome, DownloadPlan, SubtitleDownload};
use crate::domain::media_manifest::redact_sensitive_url;
use crate::domain::media_source::SubtitleTrack;
use crate::domain::selection::{select_stream, select_stream_pair};
use crate::domain::stream::{MediaStream, StreamPayload};
use crate::downloader::cancellation::spawn_ctrl_c_listener;
use crate::downloader::http_range::{DirectDownloadStats, download_direct_http};
use crate::downloader::segment_worker::download_segmented_stream;
use crate::downloader::throttle::{RateLimiter, parse_rate_limit};
use crate::error::{RuviError, RuviResult};
use crate::resolver::ResolveContext;
use crate::resolver::registry::ResolverRegistry;
use crate::storage::{ensure_writable_destination, resolve_output_path, sanitize_filename};
use crate::telemetry::DownloadProgress;

pub async fn execute_download(
    config: &RuvidConfig,
    job: DownloadJob,
    show_progress: bool,
) -> RuviResult<DownloadOutcome> {
    if job.write_manifest.as_deref() == Some(Path::new("-")) {
        let msg = if job.to_stdout {
            "--stdout cannot be combined with --write-manifest -"
        } else {
            "writing JSON handoff manifest to stdout ('-') is unsupported; specify a file path"
        };
        return Err(RuviError::InvalidArgument(msg.to_string()));
    }

    let rate_spec = job
        .limit_rate
        .as_deref()
        .unwrap_or(&config.network.limit_rate);
    let rate_limit_bps = parse_rate_limit(rate_spec)?;

    let ctx = ResolveContext::build(
        &config.network.user_agent,
        config.network.connect_timeout_seconds,
        job.timeout_seconds,
        job.proxy.as_deref().or(config.network.proxy.as_deref()),
        &job.headers,
        job.allow_private_net || config.network.allow_private_network,
        job.retry_count,
    )?;

    let source = ResolverRegistry::resolve(&ctx, &job.url).await?;
    let ctx = if let Some(ref ua_override) = source.user_agent_override {
        ResolveContext::build(
            ua_override,
            config.network.connect_timeout_seconds,
            job.timeout_seconds,
            job.proxy.as_deref().or(config.network.proxy.as_deref()),
            &job.headers,
            job.allow_private_net || config.network.allow_private_network,
            job.retry_count,
        )?
    } else {
        ctx
    };

    // Check if a dual-stream (adaptive video + audio) pair should be selected
    let dual_stream_pair = if !job.to_stdout {
        select_stream_pair(&source.streams, &job.format_spec)
    } else {
        None
    };

    if let Some((video_stream, audio_stream)) = dual_stream_pair {
        let ext = if video_stream.container.is_empty() {
            source.default_ext.as_str()
        } else {
            video_stream.container.as_str()
        };
        let resolved = resolve_output_path(
            job.output_template.as_deref(),
            &config.download.output_dir,
            &source.title,
            ext,
            &video_stream.id,
        )?;
        ensure_writable_destination(&resolved, job.overwrite)?;
        let output_path = resolved;

        let parent = output_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let raw_stem = output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("media");
        let stem = sanitize_filename(raw_stem);
        let video_output_path = parent.join(format!("{stem}.video.part"));
        let audio_output_path = parent.join(format!("{stem}.audio.part"));

        let v_bytes = match &video_stream.payload {
            StreamPayload::DirectHttp { total_bytes, .. } => *total_bytes,
            _ => None,
        };
        let a_bytes = match &audio_stream.payload {
            StreamPayload::DirectHttp { total_bytes, .. } => *total_bytes,
            _ => None,
        };
        let combined_total = match (v_bytes, a_bytes) {
            (Some(v), Some(a)) => Some(v + a),
            _ => None,
        };

        let video_plan = DownloadPlan {
            source: source.clone(),
            stream: video_stream.clone(),
            output_path: video_output_path.clone(),
            jobs: job.jobs.max(1),
            resume: job.resume,
            overwrite: job.overwrite,
            rate_limit_bps,
            retry_count: job.retry_count,
            timeout_seconds: job.timeout_seconds,
            to_stdout: false,
        };
        let audio_plan = DownloadPlan {
            source: source.clone(),
            stream: audio_stream.clone(),
            output_path: audio_output_path.clone(),
            jobs: job.jobs.max(1),
            resume: job.resume,
            overwrite: job.overwrite,
            rate_limit_bps,
            retry_count: job.retry_count,
            timeout_seconds: job.timeout_seconds,
            to_stdout: false,
        };

        let cancel = CancellationToken::new();
        spawn_ctrl_c_listener(cancel.clone());
        let limiter = RateLimiter::new(rate_limit_bps);
        let progress = DownloadProgress::new(show_progress, combined_total, &source.title);

        let (v_stats, a_stats) = tokio::try_join!(
            dispatch_stream_download(
                &ctx,
                &video_plan,
                video_stream,
                &progress,
                &limiter,
                &cancel
            ),
            dispatch_stream_download(
                &ctx,
                &audio_plan,
                audio_stream,
                &progress,
                &limiter,
                &cancel
            ),
        )?;

        progress.finish();

        let total_downloaded = v_stats.bytes_downloaded + a_stats.bytes_downloaded;
        let downloaded_subtitles = if job.subtitles && !source.subtitles.is_empty() {
            download_matching_subtitles(
                &ctx,
                &source.subtitles,
                job.subtitles_lang.as_deref(),
                &output_path,
                job.overwrite,
            )
            .await?
        } else {
            Vec::new()
        };

        let manifest_path_str = if let Some(ref m_path) = job.write_manifest {
            write_dual_stream_handoff_manifest(
                m_path,
                &source,
                video_stream,
                audio_stream,
                &video_output_path,
                &audio_output_path,
                &output_path,
                total_downloaded,
                &downloaded_subtitles,
            )?;
            Some(m_path.to_string_lossy().into_owned())
        } else {
            None
        };

        return Ok(DownloadOutcome {
            ok: true,
            url: redact_sensitive_url(&job.url),
            title: source.title,
            stream_id: format!("{}+{}", video_stream.id, audio_stream.id),
            output_path: output_path.to_string_lossy().into_owned(),
            bytes_downloaded: total_downloaded,
            resumed: v_stats.resumed || a_stats.resumed,
            chunks_total: v_stats.chunks_total + a_stats.chunks_total,
            chunks_resumed: v_stats.chunks_resumed + a_stats.chunks_resumed,
            manifest_path: manifest_path_str,
            subtitles: downloaded_subtitles,
        });
    }

    let selected = select_stream(&source.streams, &job.format_spec)?.clone();

    let output_path = if job.to_stdout {
        PathBuf::from("-")
    } else {
        let ext = if selected.container.is_empty() {
            source.default_ext.as_str()
        } else {
            selected.container.as_str()
        };
        let resolved = resolve_output_path(
            job.output_template.as_deref(),
            &config.download.output_dir,
            &source.title,
            ext,
            &selected.id,
        )?;
        ensure_writable_destination(&resolved, job.overwrite)?;
        resolved
    };

    if job.to_stdout && !matches!(selected.payload, StreamPayload::DirectHttp { .. }) {
        return Err(RuviError::Unsupported(
            "--stdout streaming is only supported for direct HTTP single-stream downloads"
                .to_string(),
        ));
    }

    let total_bytes = match &selected.payload {
        StreamPayload::DirectHttp { total_bytes, .. } => *total_bytes,
        _ => None,
    };

    let plan = DownloadPlan {
        source: source.clone(),
        stream: selected.clone(),
        output_path: output_path.clone(),
        jobs: job.jobs.max(1),
        resume: job.resume,
        overwrite: job.overwrite,
        rate_limit_bps,
        retry_count: job.retry_count,
        timeout_seconds: job.timeout_seconds,
        to_stdout: job.to_stdout,
    };

    let cancel = CancellationToken::new();
    spawn_ctrl_c_listener(cancel.clone());
    let limiter = RateLimiter::new(plan.rate_limit_bps);
    let progress =
        DownloadProgress::new(show_progress && !job.to_stdout, total_bytes, &source.title);

    let stats =
        dispatch_stream_download(&ctx, &plan, &selected, &progress, &limiter, &cancel).await?;

    progress.finish();

    let downloaded_subtitles = if job.subtitles && !job.to_stdout && !source.subtitles.is_empty() {
        download_matching_subtitles(
            &ctx,
            &source.subtitles,
            job.subtitles_lang.as_deref(),
            &output_path,
            job.overwrite,
        )
        .await?
    } else {
        Vec::new()
    };

    let manifest_path_str = if let Some(ref m_path) = job.write_manifest {
        write_handoff_manifest(
            m_path,
            &source,
            &selected,
            &output_path,
            stats.bytes_downloaded,
            &downloaded_subtitles,
        )?;
        Some(m_path.to_string_lossy().into_owned())
    } else {
        None
    };

    Ok(DownloadOutcome {
        ok: true,
        url: redact_sensitive_url(&job.url),
        title: source.title,
        stream_id: selected.id,
        output_path: output_path.to_string_lossy().into_owned(),
        bytes_downloaded: stats.bytes_downloaded,
        resumed: stats.resumed,
        chunks_total: stats.chunks_total,
        chunks_resumed: stats.chunks_resumed,
        manifest_path: manifest_path_str,
        subtitles: downloaded_subtitles,
    })
}

async fn download_matching_subtitles(
    ctx: &ResolveContext,
    tracks: &[SubtitleTrack],
    lang_filter: Option<&str>,
    media_output: &Path,
    overwrite: bool,
) -> RuviResult<Vec<SubtitleDownload>> {
    let mut results = Vec::new();
    let parent = media_output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let raw_stem = media_output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media");
    let stem = sanitize_filename(raw_stem);

    for track in tracks {
        if let Some(wanted) = lang_filter
            && !track.language.eq_ignore_ascii_case(wanted)
        {
            continue;
        }
        let safe_lang = sanitize_filename(&track.language);
        let safe_ext = sanitize_filename(&track.format);
        let sub_path = parent.join(format!("{stem}.{safe_lang}.{safe_ext}"));
        if sub_path.exists() && !overwrite {
            continue;
        }
        let (body, _, _) = ctx.fetch_text_limited(&track.url).await?;
        std::fs::write(&sub_path, body.as_bytes())?;
        results.push(SubtitleDownload {
            language: track.language.clone(),
            path: sub_path.to_string_lossy().into_owned(),
            format: track.format.clone(),
        });
    }

    Ok(results)
}

async fn dispatch_stream_download(
    ctx: &ResolveContext,
    plan: &DownloadPlan,
    stream: &MediaStream,
    progress: &DownloadProgress,
    limiter: &RateLimiter,
    cancel: &CancellationToken,
) -> RuviResult<DirectDownloadStats> {
    match stream.payload.clone() {
        StreamPayload::DirectHttp {
            url,
            total_bytes,
            accept_ranges,
            etag,
            last_modified,
        } => {
            download_direct_http(
                ctx,
                plan,
                &url,
                total_bytes,
                accept_ranges,
                etag,
                last_modified,
                progress,
                limiter,
                cancel,
            )
            .await
        }
        StreamPayload::Hls {
            playlist_url,
            init_segment_url,
            segments,
            ..
        } => {
            download_segmented_stream(
                ctx,
                plan,
                &playlist_url,
                init_segment_url,
                segments,
                true,
                progress,
                limiter,
                cancel,
            )
            .await
        }
        StreamPayload::Dash {
            mpd_url,
            init_segment_url,
            segments,
        } => {
            download_segmented_stream(
                ctx,
                plan,
                &mpd_url,
                init_segment_url,
                segments,
                false,
                progress,
                limiter,
                cancel,
            )
            .await
        }
    }
}
