use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::stream::ContainerFormat;
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverwritePolicy {
    Reject,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HwAccelMode {
    Auto,
    None,
    Nvenc,
    Qsv,
    Amf,
    VideoToolbox,
}

impl HwAccelMode {
    pub fn from_name(name: &str) -> RuviResult<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "none" | "off" | "cpu" => Ok(Self::None),
            "nvenc" | "cuda" => Ok(Self::Nvenc),
            "qsv" => Ok(Self::Qsv),
            "amf" => Ok(Self::Amf),
            "videotoolbox" => Ok(Self::VideoToolbox),
            other => Err(RuviError::InvalidArgument(format!(
                "unknown hwaccel mode '{other}' (expected auto|none|nvenc|qsv|amf|videotoolbox)"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Nvenc => "nvenc",
            Self::Qsv => "qsv",
            Self::Amf => "amf",
            Self::VideoToolbox => "videotoolbox",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleMode {
    None,
    Copy,
    MovText,
}

impl SubtitleMode {
    pub fn from_name(name: &str) -> RuviResult<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Ok(Self::None),
            "copy" => Ok(Self::Copy),
            "mov_text" | "movtext" => Ok(Self::MovText),
            other => Err(RuviError::InvalidArgument(format!(
                "unknown subtitle mode '{other}' (expected none|copy|mov_text)"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSpec {
    pub path: PathBuf,
    pub container: ContainerFormat,
    pub overwrite: OverwritePolicy,
}
