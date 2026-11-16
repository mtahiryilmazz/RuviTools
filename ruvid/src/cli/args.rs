use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "ruvid",
    version,
    about = "RUVID — Rust Video Downloader (Direct HTTP, Range/Resume, HLS/DASH VOD CLI)",
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

    /// Path to custom ruvid.toml config file
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,

    /// Disable ANSI terminal colors
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Disable terminal progress bar
    #[arg(long, global = true)]
    pub no_progress: bool,

    /// Allow connecting to localhost/private network addresses (disabled by default for SSRF safety)
    #[arg(long, global = true)]
    pub allow_private_net: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Resolve and download media from a URL
    Get(GetArgs),

    /// Inspect available streams and metadata for a URL without downloading media
    Info(InfoArgs),

    /// Download multiple entries from an M3U or JSON playlist catalog
    Playlist(PlaylistArgs),

    /// Inspect or initialize ruvid configuration
    Config(ConfigArgs),

    /// Check environment, config, cache, and writable output directory
    Doctor,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Media or manifest URL
    #[arg(value_name = "URL")]
    pub url: String,

    /// Output file path or template (supports %(title)s, %(ext)s, %(id)s)
    #[arg(short, long, value_name = "TEMPLATE")]
    pub output: Option<PathBuf>,

    /// Stream format selector (best, worst, 1080p, 720p, or stream id)
    #[arg(short, long, value_name = "SPEC", default_value = "best")]
    pub format: String,

    /// Maximum concurrent chunks/segments per download
    #[arg(long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Resume from partial file/checkpoint if compatible
    #[arg(long, overrides_with = "no_resume")]
    pub resume: bool,

    /// Disable resuming and restart transfer from scratch
    #[arg(long)]
    pub no_resume: bool,

    /// Bandwidth limit (e.g. 5M, 500K)
    #[arg(long, value_name = "RATE")]
    pub limit_rate: Option<String>,

    /// Maximum retry attempts on transient network failures
    #[arg(long, value_name = "N")]
    pub retry: Option<u32>,

    /// HTTP request timeout in seconds
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    /// HTTP/HTTPS/SOCKS proxy URL
    #[arg(long, value_name = "URL")]
    pub proxy: Option<String>,

    /// Custom request header as KEY:VALUE (repeatable)
    #[arg(long = "header", value_name = "KEY:VALUE")]
    pub headers: Vec<String>,

    /// Overwrite existing destination file
    #[arg(long)]
    pub overwrite: bool,

    /// Write a RUVI JSON handoff manifest (schema v1) for ruvic
    #[arg(long, value_name = "PATH")]
    pub write_manifest: Option<PathBuf>,

    /// Stream direct media bytes to stdout (diagnostics remain on stderr)
    #[arg(long)]
    pub stdout: bool,

    /// Download available subtitle tracks
    #[arg(long)]
    pub subtitles: bool,

    /// Preferred subtitle language code (e.g. en, tr)
    #[arg(long, value_name = "LANG")]
    pub subtitles_lang: Option<String>,
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Media or manifest URL
    #[arg(value_name = "URL")]
    pub url: String,

    /// Show all stream details
    #[arg(long)]
    pub all_formats: bool,

    /// Request timeout in seconds
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    /// Custom request header as KEY:VALUE (repeatable)
    #[arg(long = "header", value_name = "KEY:VALUE")]
    pub headers: Vec<String>,
}

#[derive(Debug, Args)]
pub struct PlaylistArgs {
    /// Playlist catalog URL (.m3u or JSON)
    #[arg(value_name = "URL")]
    pub url: String,

    /// Item selection expression (e.g. "1-5,8")
    #[arg(long, value_name = "SPEC")]
    pub items: Option<String>,

    /// Maximum concurrent playlist items
    #[arg(long, value_name = "N", default_value_t = 2)]
    pub concurrency: usize,

    /// Continue downloading remaining items when one fails
    #[arg(long)]
    pub skip_errors: bool,

    /// Output directory or template
    #[arg(short, long, value_name = "TEMPLATE")]
    pub output: Option<PathBuf>,

    /// Stream format selector for each item
    #[arg(short, long, value_name = "SPEC", default_value = "best")]
    pub format: String,

    /// Concurrent chunks/segments per item
    #[arg(long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Overwrite existing files
    #[arg(long)]
    pub overwrite: bool,
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
    /// Write a default ruvid.toml config file
    Init {
        #[arg(long)]
        overwrite: bool,
    },
}
