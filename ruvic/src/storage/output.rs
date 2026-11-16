use std::path::Path;

use crate::domain::output_spec::OverwritePolicy;
use crate::error::{RuviError, RuviResult};

pub fn ensure_output_parent(output: &Path, overwrite: OverwritePolicy) -> RuviResult<()> {
    let output_exists = output.exists() || std::fs::symlink_metadata(output).is_ok();
    if output_exists && overwrite == OverwritePolicy::Reject {
        return Err(RuviError::Filesystem(format!(
            "output file already exists: {} (pass --overwrite to replace)",
            output.display()
        )));
    }
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|err| {
            RuviError::Filesystem(format!(
                "failed to create output directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}
