mod common;

use common::{MockHttpServer, RoutePayload};
use predicates::prelude::*;
use ruvid::assembly::write_dual_stream_handoff_manifest;
use ruvid::domain::media_manifest::RuviManifest;
use ruvid::domain::media_source::{MediaSource, SourceKind};
use ruvid::domain::selection::{select_stream, select_stream_pair};
use ruvid::domain::stream::{MediaStream, StreamKind, StreamPayload};
use ruvid::resolver::registry::{
    parse_playlist_body, parse_xml_podcast_feed, parse_youtube_playlist_data,
};
use ruvid::telemetry::DownloadProgress;
use url::Url;

#[test]
fn dynamic_progress_bar_set_total_and_total_behavior() {
    let progress = DownloadProgress::new(false, None, "test_job");
    assert_eq!(progress.total(), None);

    progress.inc(512);
    progress.set_total(2048);
    assert_eq!(progress.total(), Some(2048));

    progress.inc(1024);
    progress.set_initial(0);
    assert_eq!(progress.total(), Some(2048));

    progress.finish();
}

#[test]
fn select_stream_pair_returns_none_for_muxed_video_audio() {
    let streams = vec![
        MediaStream {
            id: "itag-18".to_string(),
            kind: StreamKind::VideoAudio,
            container: "mp4".to_string(),
            width: Some(640),
            height: Some(360),
            bandwidth_bps: Some(500_000),
            codecs: Some("avc1.42001E,mp4a.40.2".to_string()),
            frame_rate: Some("30".to_string()),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/360p.mp4".to_string(),
                total_bytes: Some(5_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
        MediaStream {
            id: "itag-22".to_string(),
            kind: StreamKind::VideoAudio,
            container: "mp4".to_string(),
            width: Some(1280),
            height: Some(720),
            bandwidth_bps: Some(1_500_000),
            codecs: Some("avc1.64001F,mp4a.40.2".to_string()),
            frame_rate: Some("30".to_string()),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/720p.mp4".to_string(),
                total_bytes: Some(15_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
    ];

    // Since only muxed streams exist, select_stream_pair must return None
    assert!(select_stream_pair(&streams, "best").is_none());
    assert!(select_stream_pair(&streams, "720p").is_none());
    assert!(select_stream_pair(&streams, "itag-22").is_none());

    // select_stream maintains 100% backwards compatibility
    let single = select_stream(&streams, "best").unwrap();
    assert_eq!(single.id, "itag-22");
}

#[test]
fn select_stream_pair_resolves_4k_8k_and_compatible_audio() {
    let streams = vec![
        // Progressive muxed 720p
        MediaStream {
            id: "itag-22".to_string(),
            kind: StreamKind::VideoAudio,
            container: "mp4".to_string(),
            width: Some(1280),
            height: Some(720),
            bandwidth_bps: Some(1_500_000),
            codecs: Some("avc1.64001F,mp4a.40.2".to_string()),
            frame_rate: Some("30".to_string()),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/720p.mp4".to_string(),
                total_bytes: Some(15_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
        // DASH 1080p MP4 AVC1
        MediaStream {
            id: "itag-137".to_string(),
            kind: StreamKind::Video,
            container: "mp4".to_string(),
            width: Some(1920),
            height: Some(1080),
            bandwidth_bps: Some(4_500_000),
            codecs: Some("avc1.640028".to_string()),
            frame_rate: Some("30".to_string()),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/1080p.mp4".to_string(),
                total_bytes: Some(45_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
        // DASH 4K WebM VP9
        MediaStream {
            id: "itag-313".to_string(),
            kind: StreamKind::Video,
            container: "webm".to_string(),
            width: Some(3840),
            height: Some(2160),
            bandwidth_bps: Some(15_000_000),
            codecs: Some("vp9".to_string()),
            frame_rate: Some("60".to_string()),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/4k.webm".to_string(),
                total_bytes: Some(150_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
        // DASH 8K WebM AV1
        MediaStream {
            id: "itag-401".to_string(),
            kind: StreamKind::Video,
            container: "webm".to_string(),
            width: Some(7680),
            height: Some(4320),
            bandwidth_bps: Some(30_000_000),
            codecs: Some("av01.0.12M.08".to_string()),
            frame_rate: Some("60".to_string()),
            language: None,
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/8k.webm".to_string(),
                total_bytes: Some(300_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
        // Audio AAC (M4A)
        MediaStream {
            id: "itag-140".to_string(),
            kind: StreamKind::Audio,
            container: "m4a".to_string(),
            width: None,
            height: None,
            bandwidth_bps: Some(128_000),
            codecs: Some("mp4a.40.2".to_string()),
            frame_rate: None,
            language: Some("en".to_string()),
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/audio.m4a".to_string(),
                total_bytes: Some(4_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
        // Audio Opus (WebM)
        MediaStream {
            id: "itag-251".to_string(),
            kind: StreamKind::Audio,
            container: "webm".to_string(),
            width: None,
            height: None,
            bandwidth_bps: Some(160_000),
            codecs: Some("opus".to_string()),
            frame_rate: None,
            language: Some("en".to_string()),
            payload: StreamPayload::DirectHttp {
                url: "http://example.com/audio.webm".to_string(),
                total_bytes: Some(5_000_000),
                accept_ranges: true,
                etag: None,
                last_modified: None,
            },
        },
    ];

    // "best" selects 8K WebM paired with Opus audio
    let (v_best, a_best) = select_stream_pair(&streams, "best").unwrap();
    assert_eq!(v_best.id, "itag-401");
    assert_eq!(a_best.id, "itag-251"); // Opus for WebM

    // "1080p" selects 1080p MP4 paired with AAC M4A audio
    let (v_1080, a_1080) = select_stream_pair(&streams, "1080p").unwrap();
    assert_eq!(v_1080.id, "itag-137");
    assert_eq!(a_1080.id, "itag-140"); // AAC for MP4

    // "4k" selects 4K WebM paired with Opus audio
    let (v_4k, a_4k) = select_stream_pair(&streams, "4k").unwrap();
    assert_eq!(v_4k.id, "itag-313");
    assert_eq!(a_4k.id, "itag-251");

    // "8k" selects 8K WebM paired with Opus audio
    let (v_8k, a_8k) = select_stream_pair(&streams, "8k").unwrap();
    assert_eq!(v_8k.id, "itag-401");
    assert_eq!(a_8k.id, "itag-251");

    // Explicit video ID itag-137 pairs with compatible audio
    let (v_exact, a_exact) = select_stream_pair(&streams, "itag-137").unwrap();
    assert_eq!(v_exact.id, "itag-137");
    assert_eq!(a_exact.id, "itag-140");

    // Explicit muxed itag-22 returns None (single stream download)
    assert!(select_stream_pair(&streams, "itag-22").is_none());

    // select_stream continues to work
    let legacy_1080 = select_stream(&streams, "1080p").unwrap();
    assert_eq!(legacy_1080.id, "itag-137");
}

#[test]
fn rss_podcast_feed_xml_parsing_with_enclosures() {
    let base_url = Url::parse("https://podcast.example.com/feed.xml").unwrap();
    let rss_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Rust Engineering Podcast</title>
    <item>
      <title><![CDATA[Episode 1: Async Concurrency & Tokio]]></title>
      <enclosure url="https://cdn.example.com/episodes/ep01.mp3" length="35000000" type="audio/mpeg"/>
    </item>
    <item>
      <title>Episode 2: High Performance Network I/O</title>
      <enclosure url="/media/ep02.mp3" length="42000000" type="audio/mpeg" />
    </item>
    <item>
      <title>Bonus: Trailer</title>
      <enclosure type="audio/mpeg" url="https://cdn.example.com/trailer.mp3"/>
    </item>
  </channel>
</rss>"#;

    let entries = parse_xml_podcast_feed(rss_xml, &base_url).unwrap();
    assert_eq!(entries.len(), 3);

    assert_eq!(entries[0].index, 1);
    assert_eq!(
        entries[0].title.as_deref(),
        Some("Episode 1: Async Concurrency & Tokio")
    );
    assert_eq!(entries[0].url, "https://cdn.example.com/episodes/ep01.mp3");

    assert_eq!(entries[1].index, 2);
    assert_eq!(
        entries[1].title.as_deref(),
        Some("Episode 2: High Performance Network I/O")
    );
    assert_eq!(entries[1].url, "https://podcast.example.com/media/ep02.mp3");

    assert_eq!(entries[2].index, 3);
    assert_eq!(entries[2].title.as_deref(), Some("Bonus: Trailer"));
    assert_eq!(entries[2].url, "https://cdn.example.com/trailer.mp3");

    // Also verify via parse_playlist_body auto-detection
    let detected = parse_playlist_body(rss_xml, &base_url, "application/rss+xml").unwrap();
    assert_eq!(detected.len(), 3);
}

#[test]
fn atom_podcast_feed_xml_parsing_with_link_enclosures() {
    let base_url = Url::parse("https://atom.example.com/podcast.atom").unwrap();
    let atom_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Developer Feed</title>
  <entry>
    <title>Stream Processing in Rust</title>
    <link rel="enclosure" type="audio/mp4" href="https://media.example.com/audio1.m4a" length="12000000"/>
  </entry>
  <entry>
    <title>Zero-Copy Parsers</title>
    <link href="audio2.m4a" rel="enclosure" type="audio/mp4"/>
  </entry>
</feed>"#;

    let entries = parse_playlist_body(atom_xml, &base_url, "application/atom+xml").unwrap();
    assert_eq!(entries.len(), 2);

    assert_eq!(entries[0].index, 1);
    assert_eq!(
        entries[0].title.as_deref(),
        Some("Stream Processing in Rust")
    );
    assert_eq!(entries[0].url, "https://media.example.com/audio1.m4a");

    assert_eq!(entries[1].index, 2);
    assert_eq!(entries[1].title.as_deref(), Some("Zero-Copy Parsers"));
    assert_eq!(entries[1].url, "https://atom.example.com/audio2.m4a");
}

#[test]
fn youtube_playlist_html_yt_initial_data_parsing() {
    let base_url = Url::parse("https://www.youtube.com/playlist?list=PL12345").unwrap();
    let yt_html = r#"
<!DOCTYPE html>
<html>
<head><title>Rust Course Playlist - YouTube</title></head>
<body>
<script>
var ytInitialData = {
  "contents": {
    "twoColumnBrowseResultsRenderer": {
      "tabs": [
        {
          "tabRenderer": {
            "content": {
              "sectionListRenderer": {
                "contents": [
                  {
                    "itemSectionRenderer": {
                      "contents": [
                        {
                          "playlistVideoListRenderer": {
                            "contents": [
                              {
                                "playlistVideoRenderer": {
                                  "videoId": "abc123xyz",
                                  "title": { "runs": [{ "text": "Lesson 1: Introduction to Rust" }] }
                                }
                              },
                              {
                                "playlistVideoRenderer": {
                                  "videoId": "def456uvw",
                                  "title": { "simpleText": "Lesson 2: Ownership & Borrowing" }
                                }
                              },
                              {
                                "playlistVideoRenderer": {
                                  "videoId": "ghi789rst",
                                  "title": { "runs": [{ "text": "Lesson 3: Advanced Traits" }] }
                                }
                              }
                            ]
                          }
                        }
                      ]
                    }
                  }
                ]
              }
            }
          }
        }
      ]
    }
  }
};
</script>
</body>
</html>
"#;

    let entries = parse_youtube_playlist_data(yt_html, &base_url).unwrap();
    assert_eq!(entries.len(), 3);

    assert_eq!(entries[0].index, 1);
    assert_eq!(
        entries[0].title.as_deref(),
        Some("Lesson 1: Introduction to Rust")
    );
    assert_eq!(entries[0].url, "https://www.youtube.com/watch?v=abc123xyz");

    assert_eq!(entries[1].index, 2);
    assert_eq!(
        entries[1].title.as_deref(),
        Some("Lesson 2: Ownership & Borrowing")
    );
    assert_eq!(entries[1].url, "https://www.youtube.com/watch?v=def456uvw");

    assert_eq!(entries[2].index, 3);
    assert_eq!(
        entries[2].title.as_deref(),
        Some("Lesson 3: Advanced Traits")
    );
    assert_eq!(entries[2].url, "https://www.youtube.com/watch?v=ghi789rst");

    // Also verify parse_playlist_body delegates to YouTube parser
    let auto_entries = parse_playlist_body(yt_html, &base_url, "text/html").unwrap();
    assert_eq!(auto_entries.len(), 3);
}

#[test]
fn dual_stream_handoff_manifest_structure() {
    let source = MediaSource {
        url: "https://www.youtube.com/watch?v=testvid".to_string(),
        resolved_url: "https://www.youtube.com/watch?v=testvid".to_string(),
        kind: SourceKind::PlatformHtml,
        title: "Test Dual Stream Video".to_string(),
        default_ext: "mp4".to_string(),
        streams: Vec::new(),
        subtitles: Vec::new(),
        user_agent_override: None,
    };

    let video_stream = MediaStream {
        id: "itag-137".to_string(),
        kind: StreamKind::Video,
        container: "mp4".to_string(),
        width: Some(1920),
        height: Some(1080),
        bandwidth_bps: Some(4_500_000),
        codecs: Some("avc1.640028".to_string()),
        frame_rate: Some("30".to_string()),
        language: None,
        payload: StreamPayload::DirectHttp {
            url: "https://googlevideo.com/videoplayback?v=1".to_string(),
            total_bytes: Some(40_000_000),
            accept_ranges: true,
            etag: None,
            last_modified: None,
        },
    };

    let audio_stream = MediaStream {
        id: "itag-140".to_string(),
        kind: StreamKind::Audio,
        container: "m4a".to_string(),
        width: None,
        height: None,
        bandwidth_bps: Some(128_000),
        codecs: Some("mp4a.40.2".to_string()),
        frame_rate: None,
        language: Some("en".to_string()),
        payload: StreamPayload::DirectHttp {
            url: "https://googlevideo.com/videoplayback?a=1".to_string(),
            total_bytes: Some(4_000_000),
            accept_ranges: true,
            etag: None,
            last_modified: None,
        },
    };

    let tmp = tempfile::tempdir().unwrap();
    let manifest_path = tmp.path().join("dual_stream.ruvi.json");
    let video_part = tmp.path().join("video.video.part");
    let audio_part = tmp.path().join("video.audio.part");
    let output_mp4 = tmp.path().join("video.mp4");

    write_dual_stream_handoff_manifest(
        &manifest_path,
        &source,
        &video_stream,
        &audio_stream,
        &video_part,
        &audio_part,
        &output_mp4,
        44_000_000,
        &[],
    )
    .unwrap();

    let manifest_json = std::fs::read_to_string(&manifest_path).unwrap();
    let parsed: RuviManifest = serde_json::from_str(&manifest_json).unwrap();

    assert_eq!(parsed.schema_version, "1");
    assert_eq!(parsed.streams.len(), 2);
    assert_eq!(parsed.streams[0].kind, "video");
    assert_eq!(parsed.streams[0].height, Some(1080));
    assert_eq!(parsed.streams[1].kind, "audio");
    assert_eq!(parsed.streams[1].codec.as_deref(), Some("mp4a.40.2"));

    let dual = parsed.dual_stream.expect("dual_stream metadata must exist");
    assert_eq!(dual.video_path, video_part.to_string_lossy());
    assert_eq!(dual.audio_path, audio_part.to_string_lossy());
}

#[tokio::test]
async fn jobs_1_probes_unknown_length_and_retains_metadata() {
    let server = MockHttpServer::start().await;
    let payload = b"SINGLE_JOB_PROBE_METADATA_RETAINED".to_vec();

    // Server sends Content-Range on bytes=0-0 probe with total size
    server.set_route(
        "/small.mp4",
        RoutePayload::bytes(payload.clone(), "video/mp4"),
    );

    let tmp = tempfile::tempdir().unwrap();
    let out_file = tmp.path().join("small.mp4");

    common::ruvid_cmd(tmp.path())
        .args([
            "get",
            &format!("{}/small.mp4", server.base_url),
            "--jobs",
            "1",
            "-o",
            out_file.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            r#""bytes_downloaded": {}"#,
            payload.len()
        )));

    assert_eq!(std::fs::read(&out_file).unwrap(), payload);
}
