use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::download_job::SubtitleDownload;
use crate::domain::media_manifest::{
    MANIFEST_SCHEMA_VERSION, ManifestOutput, ManifestProducer, ManifestSource, ManifestStream,
    ManifestSubtitle, RuviManifest, redact_sensitive_url, utc_now_rfc3339,
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
        streams: vec![ManifestStream {
            kind: stream_kind.to_string(),
            codec: selected_stream.codecs.clone(),
            width: selected_stream.width,
            height: selected_stream.height,
            language: selected_stream.language.clone(),
        }],
        subtitles: subtitles
            .iter()
            .map(|s| ManifestSubtitle {
                language: s.language.clone(),
                path: s.path.clone(),
                format: s.format.clone(),
            })
            .collect(),
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
