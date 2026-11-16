use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, ETAG, LAST_MODIFIED, RANGE};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::domain::download_job::DownloadPlan;
use crate::downloader::retry::{RetryableError, run_with_retry};
use crate::downloader::scheduler::DownloadScheduler;
use crate::downloader::throttle::RateLimiter;
use crate::error::{RuviError, RuviResult};
use crate::resolver::{ResolveContext, validate_url_security};
use crate::storage::{DownloadCheckpoint, checkpoint_path_for, commit_part_file, part_file_path};
use crate::telemetry::DownloadProgress;

#[derive(Debug, Clone, Copy)]
pub struct ChunkRange {
    pub index: usize,
    pub start: u64,
    pub end: u64,
}

impl ChunkRange {
    pub fn len(self) -> u64 {
        self.end - self.start + 1
    }

    pub fn is_empty(self) -> bool {
        self.end < self.start
    }
}

pub fn plan_chunks(total_bytes: u64, jobs: usize) -> Vec<ChunkRange> {
    if total_bytes == 0 {
        return Vec::new();
    }
    let chunk_size = if total_bytes >= 16 * 1024 && jobs > 1 {
        let target_chunks = (jobs * 2).max(2) as u64;
        (total_bytes.div_ceil(target_chunks)).clamp(4096, 2 * 1024 * 1024)
    } else {
        total_bytes
    };

    let mut chunks = Vec::new();
    let mut offset = 0u64;
    let mut idx = 0usize;
    while offset < total_bytes {
        let end = (offset + chunk_size - 1).min(total_bytes - 1);
        chunks.push(ChunkRange {
            index: idx,
            start: offset,
            end,
        });
        offset = end + 1;
        idx += 1;
    }
    chunks
}

pub fn parse_content_range_header(val: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = val.trim().strip_prefix("bytes ")?;
    let (range_part, total_part) = rest.split_once('/')?;
    let (start_s, end_s) = range_part.split_once('-')?;
    let start = start_s.trim().parse::<u64>().ok()?;
    let end = end_s.trim().parse::<u64>().ok()?;
    let total = if total_part.trim() == "*" {
        None
    } else {
        Some(total_part.trim().parse::<u64>().ok()?)
    };
    Some((start, end, total))
}

#[derive(Debug)]
pub struct DirectDownloadStats {
    pub bytes_downloaded: u64,
    pub resumed: bool,
    pub chunks_total: usize,
    pub chunks_resumed: usize,
}

#[allow(clippy::too_many_arguments)]
pub async fn download_direct_http(
    ctx: &ResolveContext,
    plan: &DownloadPlan,
    url: &str,
    total_bytes: Option<u64>,
    accept_ranges: bool,
    etag: Option<String>,
    last_modified: Option<String>,
    progress: &DownloadProgress,
    limiter: &RateLimiter,
    cancel: &CancellationToken,
) -> RuviResult<DirectDownloadStats> {
    if plan.to_stdout {
        let bytes = stream_to_stdout(
            ctx,
            url,
            total_bytes,
            plan.retry_count,
            limiter,
            progress,
            cancel,
        )
        .await?;
        return Ok(DirectDownloadStats {
            bytes_downloaded: bytes,
            resumed: false,
            chunks_total: 1,
            chunks_resumed: 0,
        });
    }

    let part_path = part_file_path(&plan.output_path);
    let ckpt_path = checkpoint_path_for(&part_path);

    let can_parallel_range =
        accept_ranges && plan.jobs > 1 && total_bytes.is_some_and(|t| t >= 16 * 1024);

    if !can_parallel_range {
        if !plan.resume {
            let _ = std::fs::remove_file(&part_path);
            let _ = std::fs::remove_file(&ckpt_path);
        }
        return download_single_stream(
            ctx,
            plan,
            url,
            total_bytes,
            &part_path,
            &ckpt_path,
            progress,
            limiter,
            cancel,
        )
        .await;
    }

    let total = total_bytes.unwrap_or(0);
    let chunks = plan_chunks(total, plan.jobs);
    let chunks_total = chunks.len();
    let chunk_size = chunks.first().map(|c| c.len()).unwrap_or(total);

    let ckpt = if plan.resume && ckpt_path.exists() {
        let loaded = DownloadCheckpoint::load(&ckpt_path)?.ok_or_else(|| {
            RuviError::Integrity("checkpoint disappeared during load".to_string())
        })?;
        loaded.verify_identity(
            url,
            &plan.stream.id,
            etag.as_deref(),
            last_modified.as_deref(),
            Some(total),
        )?;
        loaded.verify_range_plan(total, chunk_size, chunks_total)?;
        if !part_path.exists() || std::fs::metadata(&part_path)?.len() != total {
            return Err(RuviError::Integrity(
                "partial file missing or size mismatch for checkpoint; use --no-resume to restart"
                    .to_string(),
            ));
        }
        loaded
    } else {
        if part_path.exists() {
            let _ = std::fs::remove_file(&part_path);
        }
        if ckpt_path.exists() {
            let _ = std::fs::remove_file(&ckpt_path);
        }
        let fresh = DownloadCheckpoint::new(
            url,
            &plan.stream.id,
            etag.clone(),
            last_modified.clone(),
            Some(total),
            chunk_size,
            chunks_total,
        );
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part_path)?;
        file.set_len(total)?;
        fresh.save_atomic(&ckpt_path)?;
        fresh
    };

    let chunks_resumed = ckpt.completed_chunks.len();
    let already_bytes: u64 = chunks
        .iter()
        .filter(|c| ckpt.completed_chunks.contains(&c.index))
        .map(|c| c.len())
        .sum();
    if already_bytes > 0 {
        progress.set_initial(already_bytes);
    }

    let pending: Vec<ChunkRange> = chunks
        .into_iter()
        .filter(|c| !ckpt.completed_chunks.contains(&c.index))
        .collect();

    if !pending.is_empty() {
        let file = Arc::new(Mutex::new(
            OpenOptions::new().read(true).write(true).open(&part_path)?,
        ));
        let ckpt_arc = Arc::new(Mutex::new(ckpt));
        let scheduler = DownloadScheduler::new(plan.jobs);

        // Verify range behavior on the very first pending chunk before launching remaining parallel jobs
        // so servers that ignore Range (returning 200 OK) trigger a clean single-stream fallback.
        let mut iter = pending.into_iter();
        if let Some(first_chunk) = iter.next() {
            match download_one_chunk(
                ctx,
                url,
                first_chunk,
                total,
                plan.retry_count,
                limiter,
                progress,
                cancel,
            )
            .await
            {
                Ok(data) => {
                    write_chunk_and_checkpoint(&file, &ckpt_arc, &ckpt_path, first_chunk, &data)
                        .await?;
                }
                Err(ChunkResultError::ServerIgnoredRange) => {
                    let _ = std::fs::remove_file(&part_path);
                    let _ = std::fs::remove_file(&ckpt_path);
                    return download_single_stream(
                        ctx,
                        plan,
                        url,
                        total_bytes,
                        &part_path,
                        &ckpt_path,
                        progress,
                        limiter,
                        cancel,
                    )
                    .await;
                }
                Err(ChunkResultError::Fatal(err)) => return Err(err),
            }
        }

        let mut join_set = JoinSet::new();
        for chunk in iter {
            let permit = scheduler.acquire().await?;
            let ctx_c = ctx.clone();
            let url_c = url.to_string();
            let limiter_c = limiter.clone();
            let progress_c = progress.clone();
            let cancel_c = cancel.clone();
            let file_c = file.clone();
            let ckpt_c = ckpt_arc.clone();
            let ckpt_path_c = ckpt_path.clone();
            let retry_max = plan.retry_count;

            join_set.spawn(async move {
                let _permit = permit;
                let data = download_one_chunk(
                    &ctx_c,
                    &url_c,
                    chunk,
                    total,
                    retry_max,
                    &limiter_c,
                    &progress_c,
                    &cancel_c,
                )
                .await
                .map_err(|e| match e {
                    ChunkResultError::ServerIgnoredRange => RuviError::Network(
                        "server unexpectedly returned 200 instead of 206 mid-transfer".to_string(),
                    ),
                    ChunkResultError::Fatal(err) => err,
                })?;

                write_chunk_and_checkpoint(&file_c, &ckpt_c, &ckpt_path_c, chunk, &data).await
            });
        }

        while let Some(res) = join_set.join_next().await {
            res.map_err(|err| RuviError::General(format!("chunk worker panicked: {err}")))??;
        }
    }

    let final_bytes = commit_part_file(
        &part_path,
        &plan.output_path,
        Some(total),
        plan.overwrite,
        Some(&ckpt_path),
    )?;

    Ok(DirectDownloadStats {
        bytes_downloaded: final_bytes,
        resumed: chunks_resumed > 0,
        chunks_total,
        chunks_resumed,
    })
}

enum ChunkResultError {
    ServerIgnoredRange,
    Fatal(RuviError),
}

#[allow(clippy::too_many_arguments)]
async fn download_one_chunk(
    ctx: &ResolveContext,
    url: &str,
    chunk: ChunkRange,
    expected_total: u64,
    retry_count: u32,
    limiter: &RateLimiter,
    progress: &DownloadProgress,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, ChunkResultError> {
    let range_header = format!("bytes={}-{}", chunk.start, chunk.end);
    let expected_len = chunk.len() as usize;
    let res = run_with_retry(retry_count, cancel, || {
        let range_val = range_header.clone();
        async move {
            let parsed = ctx.validate_url(url).map_err(RetryableError::Permanent)?;
            let resp = ctx
                .client
                .get(parsed)
                .header(RANGE, range_val)
                .send()
                .await
                .map_err(RetryableError::from_reqwest)?;

            let final_url = resp.url().clone();
            validate_url_security(&final_url, ctx.allow_private_net)
                .map_err(RetryableError::Permanent)?;

            let status = resp.status();
            if status == StatusCode::OK {
                return Err(RetryableError::Permanent(RuviError::Integrity(
                    "SERVER_IGNORED_RANGE".to_string(),
                )));
            }
            if status != StatusCode::PARTIAL_CONTENT {
                return Err(RetryableError::from_status(status, final_url.as_str()));
            }

            let cr = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_header)
                .ok_or_else(|| {
                    RetryableError::Permanent(RuviError::Integrity(
                        "206 response missing valid Content-Range header".to_string(),
                    ))
                })?;

            if cr.0 != chunk.start || cr.1 != chunk.end {
                return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                    "Content-Range mismatch: expected {}-{}, got {}-{}",
                    chunk.start, chunk.end, cr.0, cr.1
                ))));
            }
            if let Some(total) = cr.2
                && total != expected_total
            {
                return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                    "remote total size changed mid-download ({expected_total} -> {total})"
                ))));
            }

            let mut buf = Vec::with_capacity(expected_len);
            let mut stream = resp.bytes_stream();
            while let Some(item) = stream.next().await {
                if cancel.is_cancelled() {
                    return Err(RetryableError::Permanent(RuviError::Cancelled(
                        "download cancelled".to_string(),
                    )));
                }
                let bytes = item.map_err(RetryableError::from_reqwest)?;
                limiter.acquire(bytes.len()).await;
                progress.inc(bytes.len() as u64);
                buf.extend_from_slice(&bytes);
            }

            if buf.len() != expected_len {
                return Err(RetryableError::Transient(RuviError::Integrity(format!(
                    "chunk {} length mismatch: expected {expected_len}, got {}",
                    chunk.index,
                    buf.len()
                ))));
            }

            Ok(buf)
        }
    })
    .await;

    match res {
        Ok(buf) => Ok(buf),
        Err(RuviError::Integrity(msg)) if msg == "SERVER_IGNORED_RANGE" => {
            Err(ChunkResultError::ServerIgnoredRange)
        }
        Err(err) => Err(ChunkResultError::Fatal(err)),
    }
}

async fn write_chunk_and_checkpoint(
    file_mutex: &Arc<Mutex<std::fs::File>>,
    ckpt_mutex: &Arc<Mutex<DownloadCheckpoint>>,
    ckpt_path: &Path,
    chunk: ChunkRange,
    data: &[u8],
) -> RuviResult<()> {
    {
        let mut f = file_mutex.lock().await;
        f.seek(SeekFrom::Start(chunk.start))?;
        f.write_all(data)?;
        f.flush()?;
    }
    {
        let mut ckpt = ckpt_mutex.lock().await;
        ckpt.completed_chunks.insert(chunk.index);
        ckpt.save_atomic(ckpt_path)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_single_stream(
    ctx: &ResolveContext,
    plan: &DownloadPlan,
    url: &str,
    expected_total: Option<u64>,
    part_path: &Path,
    ckpt_path: &Path,
    progress: &DownloadProgress,
    limiter: &RateLimiter,
    cancel: &CancellationToken,
) -> RuviResult<DirectDownloadStats> {
    let part_owned = part_path.to_path_buf();
    let total_downloaded = run_with_retry(plan.retry_count, cancel, || {
        let part_p = part_owned.clone();
        async move {
            let parsed = ctx.validate_url(url).map_err(RetryableError::Permanent)?;
            let resp = ctx
                .client
                .get(parsed)
                .send()
                .await
                .map_err(RetryableError::from_reqwest)?;

            let final_url = resp.url().clone();
            validate_url_security(&final_url, ctx.allow_private_net)
                .map_err(RetryableError::Permanent)?;

            let status = resp.status();
            if !status.is_success() {
                return Err(RetryableError::from_status(status, final_url.as_str()));
            }

            let resp_content_len = resp.content_length();
            if let (Some(exp), Some(actual_cl)) = (expected_total, resp_content_len)
                && exp != actual_cl
            {
                return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                    "downloaded byte count mismatch: expected {exp} bytes, got Content-Length {actual_cl} bytes"
                ))));
            }

            let resp_etag = resp
                .headers()
                .get(ETAG)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let resp_lm = resp
                .headers()
                .get(LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let _ = (resp_etag, resp_lm);

            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&part_p)
                .map_err(|err| RetryableError::Permanent(RuviError::Io(err)))?;

            let mut written = 0u64;
            let mut stream = resp.bytes_stream();
            while let Some(item) = stream.next().await {
                if cancel.is_cancelled() {
                    return Err(RetryableError::Permanent(RuviError::Cancelled(
                        "download cancelled".to_string(),
                    )));
                }
                let bytes = item.map_err(RetryableError::from_reqwest)?;
                limiter.acquire(bytes.len()).await;
                file.write_all(&bytes)
                    .map_err(|err| RetryableError::Permanent(RuviError::Io(err)))?;
                written += bytes.len() as u64;
                progress.inc(bytes.len() as u64);
            }
            file.flush()
                .map_err(|err| RetryableError::Permanent(RuviError::Io(err)))?;
            if let Some(cl) = resp_content_len
                && written != cl
            {
                return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                    "downloaded byte count mismatch: expected {cl} bytes, got {written} bytes"
                ))));
            }
            Ok(written)
        }
    })
    .await?;

    let final_bytes = commit_part_file(
        part_path,
        &plan.output_path,
        expected_total,
        plan.overwrite,
        Some(ckpt_path),
    )?;
    let _ = total_downloaded;

    Ok(DirectDownloadStats {
        bytes_downloaded: final_bytes,
        resumed: false,
        chunks_total: 1,
        chunks_resumed: 0,
    })
}

async fn stream_to_stdout(
    ctx: &ResolveContext,
    url: &str,
    expected_total: Option<u64>,
    retry_count: u32,
    limiter: &RateLimiter,
    progress: &DownloadProgress,
    cancel: &CancellationToken,
) -> RuviResult<u64> {
    use std::io::Write;
    run_with_retry(retry_count, cancel, || async move {
        let parsed = ctx.validate_url(url).map_err(RetryableError::Permanent)?;
        let resp = ctx
            .client
            .get(parsed)
            .send()
            .await
            .map_err(RetryableError::from_reqwest)?;
        let final_url = resp.url().clone();
        validate_url_security(&final_url, ctx.allow_private_net)
            .map_err(RetryableError::Permanent)?;
        if !resp.status().is_success() {
            return Err(RetryableError::from_status(
                resp.status(),
                final_url.as_str(),
            ));
        }

        let resp_content_len = resp.content_length();
        if let (Some(exp), Some(actual_cl)) = (expected_total, resp_content_len)
            && exp != actual_cl
        {
            return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                "downloaded byte count mismatch: expected {exp} bytes, got Content-Length {actual_cl} bytes"
            ))));
        }

        let mut written = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(item) = stream.next().await {
            if cancel.is_cancelled() {
                return Err(RetryableError::Permanent(RuviError::Cancelled(
                    "download cancelled".to_string(),
                )));
            }
            let bytes = item.map_err(RetryableError::from_reqwest)?;
            limiter.acquire(bytes.len()).await;
            {
                let mut stdout = std::io::stdout().lock();
                stdout
                    .write_all(&bytes)
                    .map_err(|err| RetryableError::Permanent(RuviError::Io(err)))?;
                stdout
                    .flush()
                    .map_err(|err| RetryableError::Permanent(RuviError::Io(err)))?;
            }
            written += bytes.len() as u64;
            progress.inc(bytes.len() as u64);
        }
        if let Some(expected) = expected_total.or(resp_content_len)
            && written != expected
        {
            return Err(RetryableError::Permanent(RuviError::Integrity(format!(
                "downloaded byte count mismatch: expected {expected} bytes, got {written} bytes"
            ))));
        }
        Ok(written)
    })
    .await
}
