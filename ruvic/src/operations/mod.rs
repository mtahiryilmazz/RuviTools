pub mod batch;
pub mod convert;
pub mod extract_audio;
pub mod remux;
pub mod thumbnail;
pub mod trim;

use tokio_util::sync::CancellationToken;

use crate::backend::MediaBackend;
use crate::domain::conversion_job::{ConversionJob, OperationResult};
use crate::error::RuviResult;
use crate::planner::plan_conversion;
use crate::probe::ProbeService;
use crate::storage::{
    TempOutputGuard, commit_temp_output, create_temp_output_path, ensure_output_parent,
};
use crate::telemetry::CliProgressSink;

pub async fn run_job_pipeline(
    backend: &dyn MediaBackend,
    job: ConversionJob,
    show_progress: bool,
) -> RuviResult<OperationResult> {
    let probe_svc = ProbeService::new(backend);
    let source_info = probe_svc.probe_file(&job.input).await?;
    let caps = backend.capabilities().await?;

    let temp_path = create_temp_output_path(&job.output)?;
    let plan = plan_conversion(job, &source_info, &caps, temp_path.clone())?;

    ensure_output_parent(
        &plan.final_output,
        if plan.operation == crate::domain::conversion_job::OperationKind::Remux {
            // planner already checked overwrite; ensure parent directory exists
            crate::domain::output_spec::OverwritePolicy::Overwrite
        } else {
            crate::domain::output_spec::OverwritePolicy::Overwrite
        },
    )?;

    let mut guard = TempOutputGuard::new(temp_path);
    let label = format!("{:?}", plan.operation).to_lowercase();
    let sink = CliProgressSink::new(
        show_progress,
        source_info.container.duration_seconds,
        &label,
    );
    let report = backend
        .execute(&plan, &sink, CancellationToken::new())
        .await?;
    sink.finish();

    let overwrite_policy = crate::domain::output_spec::OverwritePolicy::Overwrite;
    let (output_size_bytes, output_probe) = commit_temp_output(
        backend,
        &mut guard,
        &plan.final_output,
        plan.container,
        overwrite_policy,
    )
    .await?;

    Ok(OperationResult {
        ok: true,
        operation: plan.operation,
        input: plan.input.to_string_lossy().into_owned(),
        output: plan.final_output.to_string_lossy().into_owned(),
        container: plan.container.as_str().to_string(),
        output_size_bytes,
        selected_video_encoder: report.selected_video_encoder,
        selected_audio_encoder: report.selected_audio_encoder,
        hwaccel_used: report.hwaccel_used,
        hwaccel_fallback_reason: report.hwaccel_fallback_reason,
        duration_ms: report.duration_ms,
        output_probe,
    })
}
