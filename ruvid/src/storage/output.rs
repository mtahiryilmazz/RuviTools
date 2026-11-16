use std::path::Path;

use crate::error::{RuviError, RuviResult};

pub fn ensure_writable_destination(output: &Path, overwrite: bool) -> RuviResult<()> {
    if output.exists() && !overwrite {
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
