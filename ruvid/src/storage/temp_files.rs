use std::path::{Path, PathBuf};

pub fn part_file_path(final_output: &Path) -> PathBuf {
    let mut os = final_output.as_os_str().to_os_string();
    os.push(".part");
    PathBuf::from(os)
}

pub fn segment_temp_dir(final_output: &Path) -> PathBuf {
    let mut os = final_output.as_os_str().to_os_string();
    os.push(".segments");
    PathBuf::from(os)
}
