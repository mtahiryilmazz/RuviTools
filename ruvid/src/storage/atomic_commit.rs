use std::path::Path;

use crate::error::{RuviError, RuviResult};

pub fn commit_part_file(
    part_path: &Path,
    final_output: &Path,
    expected_bytes: Option<u64>,
    overwrite: bool,
    checkpoint_path: Option<&Path>,
) -> RuviResult<u64> {
    let meta = std::fs::metadata(part_path).map_err(|err| {
        RuviError::Integrity(format!(
            "temporary download file missing {}: {err}",
            part_path.display()
        ))
    })?;
    let actual_bytes = meta.len();

    if let Some(expected) = expected_bytes
        && actual_bytes != expected
    {
        return Err(RuviError::Integrity(format!(
            "downloaded byte count mismatch: expected {expected} bytes, got {actual_bytes} bytes"
        )));
    }

    if final_output.exists() && !overwrite {
        return Err(RuviError::Filesystem(format!(
            "output file already exists: {} (pass --overwrite to replace)",
            final_output.display()
        )));
    }

    std::fs::rename(part_path, final_output).map_err(|err| {
        RuviError::Filesystem(format!(
            "failed to commit downloaded file {} -> {}: {err}",
            part_path.display(),
            final_output.display()
        ))
    })?;

    if let Some(ckpt) = checkpoint_path
        && ckpt.exists()
    {
        let _ = std::fs::remove_file(ckpt);
    }

    Ok(actual_bytes)
}
