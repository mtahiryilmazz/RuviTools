use std::path::{Path, PathBuf};

use clap::CommandFactory;
use serde::Serialize;

use crate::backend::ffmpeg::try_locate_ffmpeg;
use crate::backend::selector::{BackendOverrides, select_backend};
use crate::cli::args::{Cli, Commands, ConfigSubcommand, PresetSubcommand};
use crate::cli::render::{
    emit_json, render_batch_human, render_operation_human, render_preset_detail_human,
    render_preset_list_human, render_probe_human,
};
use crate::config::{
    AppPaths, RuvicConfig, default_config_toml, init_config, load_config, resolve_paths,
};
use crate::domain::media_info::HandoffManifest;
use crate::error::{RuviError, RuviResult};
use crate::operations::batch::{BatchRequest, execute_batch};
use crate::operations::convert::{ConvertRequest, execute_convert};
use crate::operations::extract_audio::{ExtractAudioRequest, execute_extract_audio};
use crate::operations::remux::{RemuxRequest, execute_remux};
use crate::operations::thumbnail::{ThumbnailRequest, execute_thumbnail};
use crate::operations::trim::{TrimRequest, execute_trim};
use crate::presets::{builtin_presets, find_preset, load_preset_from_path};
use crate::probe::ProbeService;

#[derive(Debug, Serialize)]
struct DoctorReport {
    ok: bool,
    engine: String,
    native_ready: bool,
    version: &'static str,
    config_path: String,
    config_exists: bool,
    ffmpeg_found: bool,
    ffmpeg_path: Option<String>,
    ffprobe_found: bool,
    ffprobe_path: Option<String>,
    ffmpeg_version: Option<String>,
    ffprobe_version: Option<String>,
    encoders_count: usize,
    hwaccels: Vec<String>,
}

pub async fn dispatch(cli: Cli) -> RuviResult<()> {
    if let Commands::Completions { shell } = cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(shell, &mut cmd, "ruvic", &mut std::io::stdout());
        return Ok(());
    }

    let (config, paths) = match &cli.command {
        Commands::Config(cfg_cmd) if matches!(cfg_cmd.action, ConfigSubcommand::Path) => {
            let p = resolve_paths(cli.config.as_deref())?;
            (RuvicConfig::default(), p)
        }
        Commands::Config(cfg_cmd) if matches!(cfg_cmd.action, ConfigSubcommand::Init { .. }) => {
            let p = resolve_paths(cli.config.as_deref())?;
            (RuvicConfig::default(), p)
        }
        _ => load_config(cli.config.as_deref())?,
    };

    let json_mode = cli.json || config.output.json;
    let show_progress = !cli.quiet && !cli.no_progress && !json_mode && config.output.progress;
    let overrides = BackendOverrides {
        ffmpeg: cli.ffmpeg.clone(),
        ffprobe: cli.ffprobe.clone(),
        timeout_seconds: None,
    };

    match cli.command {
        Commands::Config(cfg) => handle_config(
            cfg.action,
            cli.config.as_deref(),
            &config,
            &paths,
            json_mode,
        ),
        Commands::Preset(preset_cmd) => handle_preset(preset_cmd.action, &paths, json_mode),
        Commands::Doctor => handle_doctor(&config, &paths, &overrides, json_mode).await,
        Commands::Probe(args) => {
            let input = resolve_input(args.input, args.manifest.as_deref())?;
            let backend = select_backend(&config, &overrides)?;
            let probe = ProbeService::new(backend.as_ref());
            let info = probe.probe_file(&input).await?;
            if json_mode {
                emit_json(&info);
            } else {
                render_probe_human(&info, args.streams, args.chapters);
            }
            Ok(())
        }
        Commands::Convert(args) => {
            let input = resolve_input(args.input, args.manifest.as_deref())?;
            let backend = select_backend(&config, &overrides)?;
            let res = execute_convert(
                backend.as_ref(),
                &config,
                Some(&paths.presets_dir),
                ConvertRequest {
                    input,
                    output: args.output,
                    video: args.video,
                    audio: args.audio,
                    quality: args.quality,
                    speed: args.speed,
                    preset: args.preset,
                    overwrite: args.overwrite,
                    threads: args.threads,
                    hwaccel: args.hwaccel,
                    video_stream: args.video_stream,
                    audio_stream: args.audio_stream,
                    subtitle: args.subtitle,
                },
                show_progress,
            )
            .await?;
            if json_mode {
                emit_json(&res);
            } else {
                render_operation_human(&res, cli.quiet);
            }
            Ok(())
        }
        Commands::Remux(args) => {
            let primary_input = args.input.or(args.video);
            let (input, manifest_secondary) =
                resolve_inputs(primary_input, args.manifest.as_deref())?;
            let backend = select_backend(&config, &overrides)?;
            let res = execute_remux(
                backend.as_ref(),
                &config,
                RemuxRequest {
                    input,
                    secondary_input: args.audio.or(manifest_secondary),
                    output: args.output,
                    overwrite: args.overwrite,
                    video_stream: args.video_stream,
                    audio_stream: args.audio_stream,
                },
                show_progress,
            )
            .await?;
            if json_mode {
                emit_json(&res);
            } else {
                render_operation_human(&res, cli.quiet);
            }
            Ok(())
        }
        Commands::Audio(args) => {
            let input = resolve_input(args.input, args.manifest.as_deref())?;
            let backend = select_backend(&config, &overrides)?;
            let res = execute_extract_audio(
                backend.as_ref(),
                &config,
                ExtractAudioRequest {
                    input,
                    output: args.output,
                    to: args.to,
                    copy: args.copy,
                    overwrite: args.overwrite,
                    audio_stream: args.audio_stream,
                    bitrate: args.bitrate,
                    start: None,
                    duration: None,
                },
                show_progress,
            )
            .await?;
            if json_mode {
                emit_json(&res);
            } else {
                render_operation_human(&res, cli.quiet);
            }
            Ok(())
        }
        Commands::Trim(args) => {
            let input = resolve_input(args.input, args.manifest.as_deref())?;
            let backend = select_backend(&config, &overrides)?;
            let res = execute_trim(
                backend.as_ref(),
                &config,
                TrimRequest {
                    input,
                    output: args.output,
                    start: args.start,
                    end: args.end,
                    duration: args.duration,
                    reencode: args.reencode,
                    overwrite: args.overwrite,
                },
                show_progress,
            )
            .await?;
            if json_mode {
                emit_json(&res);
            } else {
                render_operation_human(&res, cli.quiet);
            }
            Ok(())
        }
        Commands::Thumbnail(args) => {
            let input = resolve_input(args.input, args.manifest.as_deref())?;
            let backend = select_backend(&config, &overrides)?;
            let res = execute_thumbnail(
                backend.as_ref(),
                &config,
                ThumbnailRequest {
                    input,
                    output: args.output,
                    at: args.at,
                    overwrite: args.overwrite,
                },
                show_progress,
            )
            .await?;
            if json_mode {
                emit_json(&res);
            } else {
                render_operation_human(&res, cli.quiet);
            }
            Ok(())
        }
        Commands::Batch(args) => {
            let backend = select_backend(&config, &overrides)?;
            let report = execute_batch(
                backend.as_ref(),
                &config,
                Some(&paths.presets_dir),
                BatchRequest {
                    input_dir: args.input_dir,
                    output_dir: args.output,
                    preset: args.preset,
                    target_ext: args.ext,
                    overwrite: args.overwrite,
                    skip_errors: args.skip_errors,
                    hwaccel: args.hwaccel,
                },
                show_progress,
            )
            .await?;
            if json_mode {
                emit_json(&report);
            } else {
                render_batch_human(&report, cli.quiet);
            }
            Ok(())
        }
        Commands::Completions { .. } => unreachable!("handled prior to config loading"),
    }
}

fn resolve_input(input: Option<PathBuf>, manifest: Option<&Path>) -> RuviResult<PathBuf> {
    resolve_inputs(input, manifest).map(|(primary, _)| primary)
}

fn resolve_inputs(
    input: Option<PathBuf>,
    manifest: Option<&Path>,
) -> RuviResult<(PathBuf, Option<PathBuf>)> {
    if let Some(m_path) = manifest {
        let loaded = HandoffManifest::load_from_path(m_path)?;
        let primary = loaded.primary_path().map(PathBuf::from).ok_or_else(|| {
            RuviError::InvalidArgument("manifest has no primary input".to_string())
        })?;
        return Ok((primary, loaded.secondary_path().map(PathBuf::from)));
    }
    let input = input.ok_or_else(|| {
        RuviError::InvalidArgument(
            "either <INPUT> or --manifest <PATH> must be provided".to_string(),
        )
    })?;
    Ok((input, None))
}

fn handle_config(
    action: ConfigSubcommand,
    explicit: Option<&Path>,
    config: &RuvicConfig,
    paths: &AppPaths,
    json_mode: bool,
) -> RuviResult<()> {
    match action {
        ConfigSubcommand::Path => {
            if json_mode {
                #[derive(Serialize)]
                struct PathOut<'a> {
                    config_file: &'a Path,
                    presets_dir: &'a Path,
                    cache_dir: &'a Path,
                    state_dir: &'a Path,
                }
                emit_json(&PathOut {
                    config_file: &paths.config_file,
                    presets_dir: &paths.presets_dir,
                    cache_dir: &paths.cache_dir,
                    state_dir: &paths.state_dir,
                });
            } else {
                println!("{}", paths.config_file.display());
            }
            Ok(())
        }
        ConfigSubcommand::Show => {
            if json_mode {
                emit_json(config);
            } else {
                let toml_text = toml::to_string_pretty(config)
                    .unwrap_or_else(|_| default_config_toml().unwrap_or_default());
                print!("{toml_text}");
            }
            Ok(())
        }
        ConfigSubcommand::Validate => {
            if json_mode {
                #[derive(Serialize)]
                struct ValidOut<'a> {
                    ok: bool,
                    config_file: &'a Path,
                }
                emit_json(&ValidOut {
                    ok: true,
                    config_file: &paths.config_file,
                });
            } else {
                println!("Config is valid ({})", paths.config_file.display());
            }
            Ok(())
        }
        ConfigSubcommand::Init { overwrite } => {
            let written = init_config(explicit, overwrite)?;
            if json_mode {
                #[derive(Serialize)]
                struct InitOut<'a> {
                    ok: bool,
                    path: &'a Path,
                }
                emit_json(&InitOut {
                    ok: true,
                    path: &written,
                });
            } else {
                println!("Initialized config at {}", written.display());
            }
            Ok(())
        }
    }
}

fn handle_preset(action: PresetSubcommand, paths: &AppPaths, json_mode: bool) -> RuviResult<()> {
    match action {
        PresetSubcommand::List => {
            let list = builtin_presets();
            if json_mode {
                emit_json(&list);
            } else {
                render_preset_list_human(&list);
            }
            Ok(())
        }
        PresetSubcommand::Show { name } => {
            let preset = find_preset(&name, Some(&paths.presets_dir))?;
            if json_mode {
                emit_json(&preset);
            } else {
                render_preset_detail_human(&preset);
            }
            Ok(())
        }
        PresetSubcommand::Validate { path } => {
            let preset = load_preset_from_path(&path)?;
            if json_mode {
                #[derive(Serialize)]
                struct PresetValidOut {
                    ok: bool,
                    name: String,
                }
                emit_json(&PresetValidOut {
                    ok: true,
                    name: preset.name,
                });
            } else {
                println!("Preset '{}' is valid ({})", preset.name, path.display());
            }
            Ok(())
        }
    }
}

async fn handle_doctor(
    config: &RuvicConfig,
    paths: &AppPaths,
    overrides: &BackendOverrides,
    json_mode: bool,
) -> RuviResult<()> {
    let explicit_ffmpeg = overrides.has_explicit_ffmpeg(config);
    let discovered = try_locate_ffmpeg(
        overrides.ffmpeg.as_deref(),
        overrides.ffprobe.as_deref(),
        config.backend.ffmpeg_path.as_deref(),
        config.backend.ffprobe_path.as_deref(),
    );

    let (ffmpeg_version, ffprobe_version, encoders_count, hwaccels) =
        if let Ok(backend) = select_backend(config, overrides) {
            match backend.capabilities().await {
                Ok(caps) => (
                    caps.ffmpeg_version,
                    caps.ffprobe_version,
                    caps.encoders.len(),
                    caps.hwaccels,
                ),
                Err(_) => (None, None, 0, Vec::new()),
            }
        } else {
            (None, None, 0, Vec::new())
        };

    let engine = if explicit_ffmpeg {
        "ffmpeg-subprocess".to_string()
    } else {
        "native-rust".to_string()
    };

    let ok = if explicit_ffmpeg {
        discovered.ffmpeg.is_some()
            && discovered.ffprobe.is_some()
            && ffmpeg_version.is_some()
            && ffprobe_version.is_some()
    } else {
        encoders_count > 0
    };

    let report = DoctorReport {
        ok,
        engine,
        native_ready: true,
        version: env!("CARGO_PKG_VERSION"),
        config_path: paths.config_file.to_string_lossy().into_owned(),
        config_exists: paths.config_file.exists(),
        ffmpeg_found: discovered.ffmpeg.is_some(),
        ffmpeg_path: discovered.ffmpeg.map(|p| p.to_string_lossy().into_owned()),
        ffprobe_found: discovered.ffprobe.is_some(),
        ffprobe_path: discovered.ffprobe.map(|p| p.to_string_lossy().into_owned()),
        ffmpeg_version,
        ffprobe_version,
        encoders_count,
        hwaccels,
    };

    if json_mode {
        emit_json(&report);
    } else {
        println!(
            "ruvic version: {} (engine={})",
            report.version, report.engine
        );
        println!(
            "config: {} (exists={})",
            report.config_path, report.config_exists
        );
        println!(
            "backend: {} ({})",
            report
                .ffmpeg_path
                .as_deref()
                .unwrap_or("built-in pure-Rust engine"),
            report.ffmpeg_version.as_deref().unwrap_or("unavailable")
        );
        println!("encoders available: {}", report.encoders_count);
        println!(
            "hwaccels: {}",
            if report.hwaccels.is_empty() {
                "none".to_string()
            } else {
                report.hwaccels.join(", ")
            }
        );
    }

    Ok(())
}
