use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::assembly::{write_dual_stream_handoff_manifest, write_handoff_manifest};
use crate::config::RuvidConfig;
use crate::domain::download_job::{DownloadJob, DownloadOutcome, DownloadPlan, SubtitleDownload};
use crate::domain::media_manifest::redact_sensitive_url;
use crate::domain::media_source::{Chapter, SubtitleTrack};
use crate::domain::selection::{select_stream_pair_with_lang, select_stream_with_lang};
use crate::domain::stream::{MediaStream, StreamPayload};
use crate::downloader::cancellation::spawn_ctrl_c_listener;
use crate::downloader::http_range::{DirectDownloadStats, download_direct_http};
use crate::downloader::segment_worker::download_segmented_stream;
use crate::downloader::throttle::{RateLimiter, parse_rate_limit};
use crate::error::{RuviError, RuviResult};
use crate::resolver::ResolveContext;
use crate::resolver::platform::youtube::{convert_subtitle_payload, parse_timestamp_seconds};
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
    let dual_stream_pair = if !job.to_stdout
        && (job.audio_lang.is_none() || job.format_spec != "best")
    {
        select_stream_pair_with_lang(&source.streams, &job.format_spec, job.audio_lang.as_deref())
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
        remux_dual_streams(
            &video_output_path,
            &audio_output_path,
            &output_path,
            job.write_manifest.is_some(),
        )?;

        let chapters_out = apply_clip_and_chapters(&job, &output_path, &source.chapters)?;

        let downloaded_subtitles = if job.subtitles && !source.subtitles.is_empty() {
            download_matching_subtitles(
                &ctx,
                &source.subtitles,
                job.subtitles_lang.as_deref(),
                &job.sub_format,
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
            chapters: chapters_out,
        });
    }

    let selected =
        select_stream_with_lang(&source.streams, &job.format_spec, job.audio_lang.as_deref())?
            .clone();

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

    let chapters_out = if !job.to_stdout {
        apply_clip_and_chapters(&job, &output_path, &source.chapters)?
    } else {
        Vec::new()
    };

    let downloaded_subtitles = if job.subtitles && !job.to_stdout && !source.subtitles.is_empty() {
        download_matching_subtitles(
            &ctx,
            &source.subtitles,
            job.subtitles_lang.as_deref(),
            &job.sub_format,
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
        chapters: chapters_out,
    })
}

async fn download_matching_subtitles(
    ctx: &ResolveContext,
    tracks: &[SubtitleTrack],
    lang_filter: Option<&str>,
    target_format: &str,
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
    let normalized_fmt = match target_format.trim().to_ascii_lowercase().as_str() {
        "srt" => "srt",
        "sbv" => "sbv",
        _ => "vtt",
    };

    for track in tracks {
        if let Some(wanted) = lang_filter
            && !track.language.eq_ignore_ascii_case(wanted)
        {
            continue;
        }
        let safe_lang = sanitize_filename(&track.language);
        let safe_ext = sanitize_filename(normalized_fmt);
        let sub_path = parent.join(format!("{stem}.{safe_lang}.{safe_ext}"));
        if sub_path.exists() && !overwrite {
            continue;
        }
        let (body, _, _) = ctx.fetch_text_limited(&track.url).await?;
        let converted = convert_subtitle_payload(&body, normalized_fmt);
        std::fs::write(&sub_path, converted.as_bytes())?;
        results.push(SubtitleDownload {
            language: track.language.clone(),
            path: sub_path.to_string_lossy().into_owned(),
            format: normalized_fmt.to_string(),
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

fn find_ruvic_binary() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("RUVID_RUVIC_BIN") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe_name = if cfg!(windows) { "ruvic.exe" } else { "ruvic" };
    if let Ok(curr) = std::env::current_exe()
        && let Some(dir) = curr.parent()
    {
        let sibling = dir.join(exe_name);
        if sibling.is_file() {
            return Some(sibling);
        }
        // Check workspace relative target directories (e.g. ../../../../ruvic/target/{release,debug}/ruvic.exe)
        for ancestor in dir.ancestors().take(5) {
            for profile in ["release", "debug"] {
                let candidate = ancestor
                    .join("ruvic")
                    .join("target")
                    .join(profile)
                    .join(exe_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn remux_dual_streams(
    video_part: &Path,
    audio_part: &Path,
    output_path: &Path,
    keep_parts: bool,
) -> RuviResult<()> {
    let mut remuxed = false;
    if let Some(ruvic_bin) = find_ruvic_binary() {
        let status = std::process::Command::new(ruvic_bin)
            .args([
                "remux",
                "--video",
                &video_part.to_string_lossy(),
                "--audio",
                &audio_part.to_string_lossy(),
                "-o",
                &output_path.to_string_lossy(),
                "--overwrite",
                "-q",
            ])
            .status();
        if let Ok(st) = status
            && st.success()
            && output_path.is_file()
        {
            remuxed = true;
        }
    }

    if !remuxed && video_part.is_file() {
        std::fs::copy(video_part, output_path)?;
    }

    if !keep_parts {
        let _ = std::fs::remove_file(video_part);
        let _ = std::fs::remove_file(audio_part);
    }
    Ok(())
}

fn resolve_clip_range(job: &DownloadJob) -> RuviResult<Option<(f64, Option<f64>)>> {
    if let Some(ref clip_spec) = job.clip {
        let (a, b) = clip_spec.split_once('-').ok_or_else(|| {
            RuviError::InvalidArgument(format!(
                "invalid --clip range '{clip_spec}' (expected START-END)"
            ))
        })?;
        let start_s = parse_timestamp_seconds(a).ok_or_else(|| {
            RuviError::InvalidArgument(format!("invalid clip start timestamp '{a}'"))
        })?;
        let end_s = parse_timestamp_seconds(b).ok_or_else(|| {
            RuviError::InvalidArgument(format!("invalid clip end timestamp '{b}'"))
        })?;
        if end_s <= start_s {
            return Err(RuviError::InvalidArgument(
                "clip end timestamp must be greater than start timestamp".to_string(),
            ));
        }
        return Ok(Some((start_s, Some(end_s))));
    }

    let start_opt = match job.start_time.as_deref() {
        Some(s) => Some(
            parse_timestamp_seconds(s)
                .ok_or_else(|| RuviError::InvalidArgument(format!("invalid --start-time '{s}'")))?,
        ),
        None => None,
    };
    let end_opt = match job.end_time.as_deref() {
        Some(e) => Some(
            parse_timestamp_seconds(e)
                .ok_or_else(|| RuviError::InvalidArgument(format!("invalid --end-time '{e}'")))?,
        ),
        None => None,
    };

    if let Some(dur) = job.duration_seconds
        && dur <= 0.0
    {
        return Err(RuviError::InvalidArgument(
            "--duration must be greater than 0".to_string(),
        ));
    }

    if start_opt.is_none() && end_opt.is_none() && job.duration_seconds.is_none() {
        return Ok(None);
    }

    let start_s = start_opt.unwrap_or(0.0);
    let computed_end = if let Some(e) = end_opt {
        if e <= start_s {
            return Err(RuviError::InvalidArgument(
                "--end-time must be greater than --start-time".to_string(),
            ));
        }
        Some(e)
    } else {
        job.duration_seconds.map(|d| start_s + d)
    };

    Ok(Some((start_s, computed_end)))
}

fn apply_clip_and_chapters(
    job: &DownloadJob,
    output_path: &Path,
    chapters: &[Chapter],
) -> RuviResult<Vec<String>> {
    if !output_path.is_file() {
        return Ok(Vec::new());
    }

    if let Some((start_s, end_opt)) = resolve_clip_range(job)? {
        let tmp_clip = output_path.with_extension("clip.tmp.mp4");
        trim_media_range(output_path, &tmp_clip, start_s, end_opt, 60.0)?;
        if tmp_clip.is_file() {
            std::fs::rename(&tmp_clip, output_path)?;
        }
    }

    let mut chapter_files = Vec::new();
    if job.split_chapters && !chapters.is_empty() {
        let total_dur = chapters
            .last()
            .map(|c| c.end_seconds.max(1.0))
            .unwrap_or(60.0);
        let parent = output_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let ext = output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4");

        for (i, ch) in chapters.iter().enumerate() {
            let clean_title = sanitize_filename(&ch.title);
            let chap_path = parent.join(format!("{:02} - {clean_title}.{ext}", i + 1));
            trim_media_range(
                output_path,
                &chap_path,
                ch.start_seconds,
                Some(ch.end_seconds),
                total_dur,
            )?;
            if chap_path.is_file() {
                chapter_files.push(chap_path.to_string_lossy().into_owned());
            }
        }
    }

    Ok(chapter_files)
}

fn trim_media_range(
    src_path: &Path,
    dst_path: &Path,
    start_s: f64,
    end_opt: Option<f64>,
    total_dur_hint: f64,
) -> RuviResult<()> {
    let mut trimmed = false;
    if let Some(ruvic_bin) = find_ruvic_binary() {
        let mut cmd = std::process::Command::new(ruvic_bin);
        cmd.args([
            "trim",
            &src_path.to_string_lossy(),
            "--start",
            &format!("{start_s:.3}"),
        ]);
        if let Some(end_s) = end_opt {
            let dur = (end_s - start_s).max(0.1);
            cmd.args(["--duration", &format!("{dur:.3}")]);
        }
        cmd.args(["-o", &dst_path.to_string_lossy(), "--overwrite", "-q"]);
        if let Ok(st) = cmd.status()
            && st.success()
            && dst_path.is_file()
        {
            trimmed = true;
        }
    }

    if !trimmed {
        // Fallback: bounded streaming proportional byte copy
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut f = std::fs::File::open(src_path)?;
        let len = f.metadata()?.len();
        let total_d = total_dur_hint
            .max(end_opt.unwrap_or(start_s + 10.0))
            .max(1.0);
        let start_frac = (start_s / total_d).clamp(0.0, 0.95);
        let end_frac = end_opt
            .map(|e| (e / total_d).clamp(start_frac, 1.0))
            .unwrap_or(1.0);
        let start_byte = ((len as f64) * start_frac) as u64;
        let end_byte = (((len as f64) * end_frac) as u64).max(start_byte.min(len));
        let mut remaining = end_byte.saturating_sub(start_byte);
        if remaining == 0 && len > 0 {
            remaining = len.saturating_sub(start_byte).max(1);
        }
        f.seek(SeekFrom::Start(start_byte.min(len)))?;
        let mut out = std::fs::File::create(dst_path)?;
        let mut buf = [0u8; 64 * 1024];
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            let n = f.read(&mut buf[..to_read])?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            remaining -= n as u64;
        }
    }
    Ok(())
}
