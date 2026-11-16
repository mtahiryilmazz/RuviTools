use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::download_job::SubtitleDownload;
use crate::domain::media_manifest::{
    DualStreamHandoff, MANIFEST_SCHEMA_VERSION, ManifestOutput, ManifestProducer, ManifestSource,
    ManifestStream, ManifestSubtitle, RuviManifest, redact_sensitive_url, utc_now_rfc3339,
};
use crate::domain::media_source::MediaSource;
use crate::domain::stream::{MediaStream, StreamKind};
use crate::error::{RuviError, RuviResult};

pub fn write_handoff_manifest(
    manifest_path: &Path,
    source: &MediaSource,
    selected_stream: &MediaStream,
    output_path: &Path,
    bytes_downloaded: u64,
    subtitles: &[SubtitleDownload],
) -> RuviResult<()> {
    write_handoff_manifest_full(
        manifest_path,
        source,
        selected_stream,
        output_path,
        bytes_downloaded,
        subtitles,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn write_dual_stream_handoff_manifest(
    manifest_path: &Path,
    source: &MediaSource,
    video_stream: &MediaStream,
    audio_stream: &MediaStream,
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    bytes_downloaded: u64,
    subtitles: &[SubtitleDownload],
) -> RuviResult<()> {
    write_handoff_manifest_full(
        manifest_path,
        source,
        video_stream,
        output_path,
        bytes_downloaded,
        subtitles,
        Some((video_path, audio_path, audio_stream)),
    )
}

fn write_handoff_manifest_full(
    manifest_path: &Path,
    source: &MediaSource,
    selected_stream: &MediaStream,
    output_path: &Path,
    bytes_downloaded: u64,
    subtitles: &[SubtitleDownload],
    dual_stream: Option<(&Path, &Path, &MediaStream)>,
) -> RuviResult<()> {
    if manifest_path == Path::new("-") {
        return Err(RuviError::InvalidArgument(
            "writing JSON handoff manifest to stdout ('-') is unsupported; specify a file path"
                .to_string(),
        ));
    }

    if let Some(parent) = manifest_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let stream_kind = match selected_stream.kind {
        StreamKind::VideoAudio => "video+audio",
        StreamKind::Video => "video",
        StreamKind::Audio => "audio",
    };

    let mut streams = vec![ManifestStream {
        kind: stream_kind.to_string(),
        codec: selected_stream.codecs.clone(),
        width: selected_stream.width,
        height: selected_stream.height,
        language: selected_stream.language.clone(),
    }];

    let dual_stream_handoff = if let Some((v_path, a_path, audio_stream)) = dual_stream {
        streams.push(ManifestStream {
            kind: "audio".to_string(),
            codec: audio_stream.codecs.clone(),
            width: audio_stream.width,
            height: audio_stream.height,
            language: audio_stream.language.clone(),
        });
        Some(DualStreamHandoff {
            video_path: v_path.to_string_lossy().into_owned(),
            audio_path: a_path.to_string_lossy().into_owned(),
        })
    } else {
        None
    };

    let manifest = RuviManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        producer: ManifestProducer {
            name: "ruvid".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        source: ManifestSource {
            url: redact_sensitive_url(&source.url),
            title: Some(source.title.clone()),
        },
        output: ManifestOutput {
            path: output_path.to_string_lossy().into_owned(),
            container: selected_stream.container.clone(),
            size_bytes: bytes_downloaded,
        },
        streams,
        subtitles: subtitles
            .iter()
            .map(|s| ManifestSubtitle {
                language: s.language.clone(),
                path: s.path.clone(),
                format: s.format.clone(),
            })
            .collect(),
        dual_stream: dual_stream_handoff,
        created_at: utc_now_rfc3339(),
        extra: BTreeMap::new(),
    };

    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|err| RuviError::General(format!("failed serializing manifest: {err}")))?;

    let mut tmp_os = manifest_path.as_os_str().to_os_string();
    tmp_os.push(".tmp");
    let tmp_path = std::path::PathBuf::from(tmp_os);
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(&tmp_path, manifest_path)?;

    Ok(())
}
