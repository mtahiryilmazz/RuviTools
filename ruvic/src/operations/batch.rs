use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backend::MediaBackend;
use crate::config::RuvicConfig;
use crate::domain::conversion_job::OperationResult;
use crate::error::{RuviError, RuviResult};
use crate::operations::convert::{ConvertRequest, execute_convert};
use crate::presets::find_preset;

#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub preset: Option<String>,
    pub target_ext: Option<String>,
    pub overwrite: bool,
    pub skip_errors: bool,
    pub hwaccel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItemError {
    pub input: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchReport {
    pub ok: bool,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<OperationResult>,
    pub errors: Vec<BatchItemError>,
}

const SUPPORTED_MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "ts", "avi", "m4v", "m4a", "mp3", "flac", "wav", "ogg",
];

pub async fn execute_batch(
    backend: &dyn MediaBackend,
    config: &RuvicConfig,
    user_presets_dir: Option<&Path>,
    req: BatchRequest,
    show_progress: bool,
) -> RuviResult<BatchReport> {
    if !req.input_dir.is_dir() {
        return Err(RuviError::Filesystem(format!(
            "batch input path is not a directory: {}",
            req.input_dir.display()
        )));
    }
    std::fs::create_dir_all(&req.output_dir)?;

    let out_ext = if let Some(ref ext) = req.target_ext {
        ext.trim_start_matches('.').to_string()
    } else if let Some(ref preset_name) = req.preset {
        let preset = find_preset(preset_name, user_presets_dir)?;
        preset.container.format
    } else {
        "mp4".to_string()
    };

    let mut inputs: Vec<PathBuf> = std::fs::read_dir(&req.input_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().and_then(|e| e.to_str()).is_some_and(|ext| {
                    SUPPORTED_MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
                })
        })
        .collect();
    inputs.sort();

    let total = inputs.len();
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for input_path in inputs {
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let output_path = req.output_dir.join(format!("{stem}.{out_ext}"));
        let convert_req = ConvertRequest {
            input: input_path.clone(),
            output: output_path,
            video: None,
            audio: None,
            quality: None,
            speed: None,
            preset: req.preset.clone(),
            overwrite: req.overwrite,
            threads: None,
            hwaccel: req.hwaccel.clone(),
            video_stream: None,
            audio_stream: None,
            subtitle: None,
        };
        match execute_convert(
            backend,
            config,
            user_presets_dir,
            convert_req,
            show_progress,
        )
        .await
        {
            Ok(res) => results.push(res),
            Err(err) => {
                if !req.skip_errors {
                    return Err(err);
                }
                errors.push(BatchItemError {
                    input: input_path.to_string_lossy().into_owned(),
                    error: err.to_string(),
                });
            }
        }
    }

    let succeeded = results.len();
    let failed = errors.len();
    Ok(BatchReport {
        ok: failed == 0,
        total,
        succeeded,
        failed,
        results,
        errors,
    })
}
