use crate::domain::preset::{Preset, PresetAudio, PresetContainer, PresetVideo};

pub fn builtin_presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "web-mp4".to_string(),
            description: "Web uyumlu H.264/AAC MP4 çıktı profili".to_string(),
            container: PresetContainer {
                format: "mp4".to_string(),
                faststart: true,
            },
            video: Some(PresetVideo {
                codec: "libx264".to_string(),
                quality: Some(23),
                preset: Some("medium".to_string()),
                pixel_format: Some("yuv420p".to_string()),
                max_height: None,
            }),
            audio: Some(PresetAudio {
                codec: "aac".to_string(),
                bitrate: Some("192k".to_string()),
            }),
        },
        Preset {
            name: "archive-mkv".to_string(),
            description: "Yüksek kaliteli HEVC/Opus MKV arşiv profili".to_string(),
            container: PresetContainer {
                format: "mkv".to_string(),
                faststart: false,
            },
            video: Some(PresetVideo {
                codec: "libx265".to_string(),
                quality: Some(20),
                preset: Some("slow".to_string()),
                pixel_format: Some("yuv420p".to_string()),
                max_height: None,
            }),
            audio: Some(PresetAudio {
                codec: "libopus".to_string(),
                bitrate: Some("160k".to_string()),
            }),
        },
        Preset {
            name: "fast-720p".to_string(),
            description: "Hızlı 720p H.264/AAC MP4 önizleme profili".to_string(),
            container: PresetContainer {
                format: "mp4".to_string(),
                faststart: true,
            },
            video: Some(PresetVideo {
                codec: "libx264".to_string(),
                quality: Some(25),
                preset: Some("veryfast".to_string()),
                pixel_format: Some("yuv420p".to_string()),
                max_height: Some(720),
            }),
            audio: Some(PresetAudio {
                codec: "aac".to_string(),
                bitrate: Some("128k".to_string()),
            }),
        },
        Preset {
            name: "audio-mp3".to_string(),
            description: "Ses çıkarma için 192 kbps MP3 profili".to_string(),
            container: PresetContainer {
                format: "mp3".to_string(),
                faststart: false,
            },
            video: None,
            audio: Some(PresetAudio {
                codec: "libmp3lame".to_string(),
                bitrate: Some("192k".to_string()),
            }),
        },
        Preset {
            name: "audio-flac".to_string(),
            description: "Kayıpsız FLAC ses arşiv profili".to_string(),
            container: PresetContainer {
                format: "flac".to_string(),
                faststart: false,
            },
            video: None,
            audio: Some(PresetAudio {
                codec: "flac".to_string(),
                bitrate: None,
            }),
        },
    ]
}

pub fn find_builtin_preset(name: &str) -> Option<Preset> {
    builtin_presets()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
}
