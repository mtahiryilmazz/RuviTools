use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "ruvic",
    version,
    about = "RUVIC — Rust Video Converter (FFmpeg/ffprobe media processing CLI)",
    propagate_version = true
)]
pub struct Cli {
    /// Increase diagnostic verbosity (-v, -vv)
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress non-essential human output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Emit machine-readable JSON output to stdout
    #[arg(long, global = true)]
    pub json: bool,

    /// Path to custom ruvic.toml config file
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,

    /// Explicit path to ffmpeg executable
    #[arg(long, value_name = "PATH", env = "RUVIC_FFMPEG_PATH", global = true)]
    pub ffmpeg: Option<PathBuf>,

    /// Explicit path to ffprobe executable
    #[arg(long, value_name = "PATH", env = "RUVIC_FFPROBE_PATH", global = true)]
    pub ffprobe: Option<PathBuf>,

    /// Disable ANSI terminal colors
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Disable terminal progress bar
    #[arg(long, global = true)]
    pub no_progress: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Inspect media container, streams, and chapters
    Probe(ProbeArgs),

    /// Transcode media using explicit codecs or a preset
    Convert(ConvertArgs),

    /// Remux streams into another container without re-encoding
    Remux(RemuxArgs),

    /// Extract or convert audio track from media
    Audio(AudioArgs),

    /// Trim a time range from a media file
    Trim(TrimArgs),

    /// Capture a thumbnail image at a given timestamp
    Thumbnail(ThumbnailArgs),

    /// Convert all supported media files in a directory
    Batch(BatchArgs),

    /// List, inspect, or validate conversion presets
    Preset(PresetArgs),

    /// Inspect or initialize ruvic configuration
    Config(ConfigArgs),

    /// Check ffmpeg/ffprobe availability, versions, and capabilities
    Doctor,
}

#[derive(Debug, Args)]
pub struct ProbeArgs {
    /// Input media file path
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Read input file path from a RUVI JSON handoff manifest
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Show detailed stream table
    #[arg(long)]
    pub streams: bool,

    /// Show chapters if present
    #[arg(long)]
    pub chapters: bool,

    /// Show container format summary
    #[arg(long)]
    pub format: bool,
}

#[derive(Debug, Args)]
pub struct ConvertArgs {
    /// Input media file path
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Read input file path from a RUVI JSON handoff manifest
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Destination output file path
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Video codec (h264, h265, vp9, av1, copy)
    #[arg(long, value_name = "CODEC")]
    pub video: Option<String>,

    /// Audio codec (aac, mp3, opus, flac, copy)
    #[arg(long, value_name = "CODEC")]
    pub audio: Option<String>,

    /// Codec-specific quality value (e.g. CRF 0-51 for h264/h265, 0-63 for vp9/av1)
    #[arg(long, value_name = "VALUE")]
    pub quality: Option<u8>,

    /// Encoder speed/compression preset (ultrafast..veryslow)
    #[arg(long, value_name = "PRESET")]
    pub speed: Option<String>,

    /// Named built-in or custom preset
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,

    /// Allow overwriting existing output file
    #[arg(long)]
    pub overwrite: bool,

    /// Encoder thread count
    #[arg(long, value_name = "N")]
    pub threads: Option<u32>,

    /// Hardware acceleration mode (auto, none, nvenc, qsv, amf, videotoolbox)
    #[arg(long, value_name = "MODE")]
    pub hwaccel: Option<String>,

    /// Explicit video stream index to map
    #[arg(long, value_name = "INDEX")]
    pub video_stream: Option<usize>,

    /// Explicit audio stream index to map
    #[arg(long, value_name = "INDEX")]
    pub audio_stream: Option<usize>,

    /// Subtitle handling mode (none, copy, mov_text)
    #[arg(long, value_name = "MODE")]
    pub subtitle: Option<String>,
}

#[derive(Debug, Args)]
pub struct RemuxArgs {
    /// Input media file path
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Read input file path from a RUVI JSON handoff manifest
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Destination output file path
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Explicitly enforce stream copy mode (always enabled for remux)
    #[arg(long)]
    pub copy: bool,

    /// Allow overwriting existing output file
    #[arg(long)]
    pub overwrite: bool,

    /// Explicit video stream index
    #[arg(long, value_name = "INDEX")]
    pub video_stream: Option<usize>,

    /// Explicit audio stream index
    #[arg(long, value_name = "INDEX")]
    pub audio_stream: Option<usize>,
}

#[derive(Debug, Args)]
pub struct AudioArgs {
    /// Input media file path
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Read input file path from a RUVI JSON handoff manifest
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Target audio format/codec (mp3, flac, aac, wav, opus)
    #[arg(long, value_name = "FMT")]
    pub to: Option<String>,

    /// Copy audio stream without re-encoding
    #[arg(long)]
    pub copy: bool,

    /// Destination output audio file path
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Allow overwriting existing output file
    #[arg(long)]
    pub overwrite: bool,

    /// Explicit audio stream index
    #[arg(long, value_name = "INDEX")]
    pub audio_stream: Option<usize>,
}

#[derive(Debug, Args)]
pub struct TrimArgs {
    /// Input media file path
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Read input file path from a RUVI JSON handoff manifest
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Start timestamp (e.g. 00:00:10 or 10.5)
    #[arg(long, value_name = "TIME")]
    pub start: String,

    /// End timestamp (e.g. 00:00:35)
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,

    /// Duration in seconds from start
    #[arg(long, value_name = "SECONDS")]
    pub duration: Option<f64>,

    /// Re-encode for frame-accurate cutting instead of stream copy
    #[arg(long)]
    pub reencode: bool,

    /// Destination output clip path
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Allow overwriting existing output file
    #[arg(long)]
    pub overwrite: bool,
}

#[derive(Debug, Args)]
pub struct ThumbnailArgs {
    /// Input media file path
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Read input file path from a RUVI JSON handoff manifest
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,

    /// Timestamp at which to capture the thumbnail (e.g. 00:00:05)
    #[arg(long, value_name = "TIME")]
    pub at: String,

    /// Destination image path (.jpg, .png, .webp)
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Allow overwriting existing output file
    #[arg(long)]
    pub overwrite: bool,
}

#[derive(Debug, Args)]
pub struct BatchArgs {
    /// Directory containing source media files
    #[arg(value_name = "INPUT_DIR")]
    pub input_dir: PathBuf,

    /// Destination directory for converted files
    #[arg(short, long, value_name = "OUTPUT_DIR")]
    pub output: PathBuf,

    /// Preset name or path
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,

    /// Target output extension (default from preset or mp4)
    #[arg(long, value_name = "EXT")]
    pub ext: Option<String>,

    /// Allow overwriting existing files in output directory
    #[arg(long)]
    pub overwrite: bool,

    /// Continue processing remaining files if one fails
    #[arg(long)]
    pub skip_errors: bool,

    /// Hardware acceleration mode
    #[arg(long, value_name = "MODE")]
    pub hwaccel: Option<String>,
}

#[derive(Debug, Args)]
pub struct PresetArgs {
    #[command(subcommand)]
    pub action: PresetSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum PresetSubcommand {
    /// List all built-in presets
    List,
    /// Show details of a built-in or user preset
    Show { name: String },
    /// Validate a TOML preset file
    Validate { path: PathBuf },
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Print effective config file path
    Path,
    /// Print effective configuration
    Show,
    /// Validate configuration file
    Validate,
    /// Write a default ruvic.toml config file
    Init {
        #[arg(long)]
        overwrite: bool,
    },
}
