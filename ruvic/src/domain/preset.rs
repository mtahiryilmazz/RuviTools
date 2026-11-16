use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub description: String,
    pub container: PresetContainer,
    #[serde(default)]
    pub video: Option<PresetVideo>,
    #[serde(default)]
    pub audio: Option<PresetAudio>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetContainer {
    pub format: String,
    #[serde(default)]
    pub faststart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetVideo {
    pub codec: String,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetAudio {
    pub codec: String,
    #[serde(default)]
    pub bitrate: Option<String>,
}
