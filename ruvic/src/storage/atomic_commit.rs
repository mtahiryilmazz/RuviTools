use std::path::Path;

use crate::backend::MediaBackend;
use crate::domain::media_info::MediaInfo;
use crate::domain::output_spec::OverwritePolicy;
use crate::domain::stream::ContainerFormat;
use crate::error::{RuviError, RuviResult};
use crate::storage::temp_files::TempOutputGuard;

/// Validates the generated temp output (non-empty, plus ffprobe check for media containers)
/// and atomically renames it to the final destination.
pub async fn commit_temp_output(
    backend: &dyn MediaBackend,
    guard: &mut TempOutputGuard,
    final_output: &Path,
    container: ContainerFormat,
    overwrite: OverwritePolicy,
) -> RuviResult<(u64, Option<MediaInfo>)> {
    let temp_path = guard.path().to_path_buf();
    let meta = std::fs::metadata(&temp_path).map_err(|err| {
        RuviError::Integrity(format!(
            "temporary output missing {}: {err}",
            temp_path.display()
        ))
    })?;
    if meta.len() == 0 {
        return Err(RuviError::Integrity(format!(
            "produced output file is empty: {}",
            temp_path.display()
        )));
    }

    let probed = if container.is_image() {
        None
    } else {
        let info = backend.probe(&temp_path).await.map_err(|err| {
            RuviError::Integrity(format!(
                "output validation failed on {}: {err}",
                temp_path.display()
            ))
        })?;
        if info.streams.is_empty() {
            return Err(RuviError::Integrity(format!(
                "output validation failed: no media streams found in {}",
                temp_path.display()
            )));
        }
        Some(info)
    };

    if final_output.exists() && overwrite == OverwritePolicy::Reject {
        return Err(RuviError::Filesystem(format!(
            "destination appeared before commit: {}",
            final_output.display()
        )));
    }

    std::fs::rename(&temp_path, final_output).map_err(|err| {
        RuviError::Filesystem(format!(
            "failed to commit output {} -> {}: {err}",
            temp_path.display(),
            final_output.display()
        ))
    })?;
    guard.mark_committed();

    Ok((meta.len(), probed))
}
