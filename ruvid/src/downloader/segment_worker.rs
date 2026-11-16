use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::assembly::segment_join::join_segments_in_order;
use crate::domain::download_job::DownloadPlan;
use crate::domain::stream::MediaSegment;
use crate::downloader::http_range::DirectDownloadStats;
use crate::downloader::retry::{RetryableError, run_with_retry};
use crate::downloader::scheduler::DownloadScheduler;
use crate::downloader::throttle::RateLimiter;
use crate::error::{RuviError, RuviResult};
use crate::resolver::hls::fetch_and_parse_media_playlist;
use crate::resolver::{ResolveContext, validate_url_security};
use crate::storage::{DownloadCheckpoint, commit_part_file, part_file_path, segment_temp_dir};
use crate::telemetry::DownloadProgress;

#[allow(clippy::too_many_arguments)]
pub async fn download_segmented_stream(
    ctx: &ResolveContext,
    plan: &DownloadPlan,
    source_manifest_url: &str,
    mut init_segment_url: Option<String>,
    mut segments: Vec<MediaSegment>,
    is_hls: bool,
    progress: &DownloadProgress,
    limiter: &RateLimiter,
    cancel: &CancellationToken,
) -> RuviResult<DirectDownloadStats> {
    if is_hls && segments.is_empty() {
        let parsed = fetch_and_parse_media_playlist(ctx, source_manifest_url).await?;
        init_segment_url = parsed.init_segment_url;
        segments = parsed.segments;
    }

    if segments.is_empty() {
        return Err(RuviError::Unsupported(
            "segmented stream resolved with zero segments".to_string(),
        ));
    }

    let seg_dir = segment_temp_dir(&plan.output_path);
    if !plan.resume && seg_dir.exists() {
        let _ = std::fs::remove_dir_all(&seg_dir);
    }
    std::fs::create_dir_all(&seg_dir)?;

    let ckpt_path = seg_dir.join("checkpoint.ruvid.json");
    let total_segs = segments.len();

    let ckpt = if plan.resume && ckpt_path.exists() {
        let loaded = DownloadCheckpoint::load(&ckpt_path)?
            .ok_or_else(|| RuviError::Integrity("segment checkpoint disappeared".to_string()))?;
        loaded.verify_identity(source_manifest_url, &plan.stream.id, None, None, None)?;
        loaded.verify_segment_plan(total_segs)?;
        loaded
    } else {
        let fresh = DownloadCheckpoint::new(
            source_manifest_url,
            &plan.stream.id,
            None,
            None,
            None,
            0,
            total_segs,
        );
        fresh.save_atomic(&ckpt_path)?;
        fresh
    };

    let init_path = seg_dir.join("init.bin");
    if let Some(ref init_url) = init_segment_url
        && (!plan.resume || !init_path.exists())
    {
        let bytes = fetch_segment_bytes(
            ctx,
            init_url,
            None,
            plan.retry_count,
            limiter,
            progress,
            cancel,
        )
        .await?;
        std::fs::write(&init_path, &bytes)?;
    }

    let segments_resumed = ckpt.completed_segments.len();
    let pending: Vec<MediaSegment> = segments
        .iter()
        .filter(|s| {
            let p = segment_file_path(&seg_dir, s.index);
            !(ckpt.completed_segments.contains(&s.index) && p.exists())
        })
        .cloned()
        .collect();

    if !pending.is_empty() {
        let ckpt_arc = Arc::new(Mutex::new(ckpt));
        let scheduler = DownloadScheduler::new(plan.jobs);
        let mut join_set = JoinSet::new();

        for seg in pending {
            let permit = scheduler.acquire().await?;
            let ctx_c = ctx.clone();
            let seg_dir_c = seg_dir.clone();
            let ckpt_c = ckpt_arc.clone();
            let ckpt_path_c = ckpt_path.clone();
            let limiter_c = limiter.clone();
            let progress_c = progress.clone();
            let cancel_c = cancel.clone();
            let retry_max = plan.retry_count;

            join_set.spawn(async move {
                let _permit = permit;
                let data = fetch_segment_bytes(
                    &ctx_c,
                    &seg.url,
                    seg.byte_range,
                    retry_max,
                    &limiter_c,
                    &progress_c,
                    &cancel_c,
                )
                .await?;
                let seg_path = segment_file_path(&seg_dir_c, seg.index);
                let tmp_seg = seg_dir_c.join(format!("seg_{:06}.bin.tmp", seg.index));
                std::fs::write(&tmp_seg, &data)?;
                std::fs::rename(&tmp_seg, &seg_path)?;

                let mut guard = ckpt_c.lock().await;
                guard.completed_segments.insert(seg.index);
                guard.save_atomic(&ckpt_path_c)?;
                Ok::<(), RuviError>(())
            });
        }

        while let Some(res) = join_set.join_next().await {
            res.map_err(|err| RuviError::General(format!("segment worker panicked: {err}")))??;
        }
    }

    let ordered_files: Vec<std::path::PathBuf> = (0..total_segs)
        .map(|i| segment_file_path(&seg_dir, i))
        .collect();

    let init_opt = init_segment_url.as_ref().map(|_| init_path.as_path());
    let part_path = part_file_path(&plan.output_path);
    let assembled_bytes = join_segments_in_order(init_opt, &ordered_files, &part_path)?;

    let final_bytes = commit_part_file(
        &part_path,
        &plan.output_path,
        Some(assembled_bytes),
        plan.overwrite,
        None,
    )?;

    let _ = std::fs::remove_dir_all(&seg_dir);

    Ok(DirectDownloadStats {
        bytes_downloaded: final_bytes,
        resumed: segments_resumed > 0,
        chunks_total: total_segs,
        chunks_resumed: segments_resumed,
    })
}

pub fn segment_file_path(seg_dir: &Path, index: usize) -> std::path::PathBuf {
    seg_dir.join(format!("seg_{index:06}.bin"))
}

pub async fn fetch_segment_bytes(
    ctx: &ResolveContext,
    url: &str,
    byte_range: Option<crate::domain::stream::ByteRangeSpec>,
    retry_count: u32,
    limiter: &RateLimiter,
    progress: &DownloadProgress,
    cancel: &CancellationToken,
) -> RuviResult<Vec<u8>> {
    run_with_retry(retry_count, cancel, || async move {
        let parsed = ctx.validate_url(url).map_err(RetryableError::Permanent)?;
        let mut req = ctx.client.get(parsed);
        if let Some(br) = byte_range {
            let end = br.offset + br.length.saturating_sub(1);
            req = req.header(RANGE, format!("bytes={}-{}", br.offset, end));
        }

        let resp = req.send().await.map_err(RetryableError::from_reqwest)?;
        let final_url = resp.url().clone();
        validate_url_security(&final_url, ctx.allow_private_net)
            .map_err(RetryableError::Permanent)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(RetryableError::from_status(status, final_url.as_str()));
        }

        if let Some(br) = byte_range
            && status == StatusCode::PARTIAL_CONTENT
        {
            let expected_end = br.offset + br.length.saturating_sub(1);
            let cr = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(crate::downloader::http_range::parse_content_range_header)
                .ok_or_else(|| {
                    RetryableError::Permanent(RuviError::Integrity(
                        "206 segment response missing valid Content-Range header".to_string(),
                    ))
                })?;
            if cr.0 != br.offset || cr.1 != expected_end {
                return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                    "segment Content-Range mismatch: expected {}-{expected_end}, got {}-{}",
                    br.offset, cr.0, cr.1
                ))));
            }
        }

        let resp_content_len = resp.content_length();
        let mut buf = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(item) = stream.next().await {
            if cancel.is_cancelled() {
                return Err(RetryableError::Permanent(RuviError::Cancelled(
                    "download cancelled".to_string(),
                )));
            }
            let chunk = item.map_err(RetryableError::from_reqwest)?;
            limiter.acquire(chunk.len()).await;
            progress.inc(chunk.len() as u64);
            buf.extend_from_slice(&chunk);
        }

        if let Some(cl) = resp_content_len
            && buf.len() as u64 != cl
        {
            return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                "segment byte count mismatch: expected {cl} bytes, got {} bytes",
                buf.len()
            ))));
        }

        if let Some(br) = byte_range {
            if status == StatusCode::OK {
                let start = usize::try_from(br.offset).map_err(|_| {
                    RetryableError::Permanent(RuviError::Integrity(
                        "segment byte-range offset overflow".to_string(),
                    ))
                })?;
                let end = br
                    .offset
                    .checked_add(br.length)
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| {
                        RetryableError::Permanent(RuviError::Integrity(
                            "segment byte-range end overflow".to_string(),
                        ))
                    })?;
                if buf.len() < end {
                    return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                        "segment byte-range exceeds response size: need {end}, got {}",
                        buf.len()
                    ))));
                }
                return Ok(buf[start..end].to_vec());
            } else if buf.len() as u64 != br.length {
                return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                    "segment byte-range length mismatch: expected {}, got {}",
                    br.length,
                    buf.len()
                ))));
            }
        }

        Ok(buf)
    })
    .await
}
