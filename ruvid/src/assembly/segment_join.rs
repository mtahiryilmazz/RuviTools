use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{RuviError, RuviResult};

pub fn join_segments_in_order(
    init_segment: Option<&Path>,
    ordered_segments: &[PathBuf],
    dest_part_file: &Path,
) -> RuviResult<u64> {
    if let Some(parent) = dest_part_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest_part_file)?;
    let mut writer = BufWriter::new(file);
    let mut total_written = 0u64;

    if let Some(init_path) = init_segment {
        total_written += copy_file_into(init_path, &mut writer)?;
    }

    for seg_path in ordered_segments {
        if !seg_path.exists() {
            return Err(RuviError::Integrity(format!(
                "missing segment during assembly: {}",
                seg_path.display()
            )));
        }
        total_written += copy_file_into(seg_path, &mut writer)?;
    }

    writer.flush()?;
    Ok(total_written)
}

fn copy_file_into(src: &Path, writer: &mut BufWriter<File>) -> RuviResult<u64> {
    let mut reader = File::open(src).map_err(|err| {
        RuviError::Integrity(format!("failed opening segment {}: {err}", src.display()))
    })?;
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        written += n as u64;
    }
    Ok(written)
}
