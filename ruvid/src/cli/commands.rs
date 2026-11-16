use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::cli::args::{Cli, Commands, ConfigSubcommand, GetArgs, InfoArgs, PlaylistArgs};
use crate::cli::render::{
    emit_json, render_download_human, render_playlist_human, render_source_info_human,
};
use crate::config::{
    AppPaths, RuvidConfig, default_config_toml, init_config, load_config, resolve_paths,
};
use crate::domain::download_job::{DownloadJob, PlaylistItemError, PlaylistReport};
use crate::domain::media_manifest::{
    redact_media_source, redact_sensitive_url, redact_urls_in_text,
};
use crate::downloader::execute_download;
use crate::downloader::scheduler::DownloadScheduler;
use crate::error::{RuviError, RuviResult};
use crate::resolver::ResolveContext;
use crate::resolver::registry::{ResolverRegistry, fetch_playlist_entries, parse_item_selection};
use crate::storage::sanitize_filename;

#[derive(Debug, Serialize)]
struct DoctorReport {
    ok: bool,
    version: &'static str,
    config_path: String,
    config_exists: bool,
    cache_dir: String,
    state_dir: String,
    output_dir: String,
    output_dir_writable: bool,
}

pub async fn dispatch(cli: Cli) -> RuviResult<()> {
    let (config, paths) = match &cli.command {
        Commands::Config(cfg_cmd) if matches!(cfg_cmd.action, ConfigSubcommand::Path) => {
            let p = resolve_paths(cli.config.as_deref())?;
            (RuvidConfig::default(), p)
        }
        Commands::Config(cfg_cmd) if matches!(cfg_cmd.action, ConfigSubcommand::Init { .. }) => {
            let p = resolve_paths(cli.config.as_deref())?;
            (RuvidConfig::default(), p)
        }
        _ => load_config(cli.config.as_deref())?,
    };

    let json_mode = cli.json || config.output.json;
    let show_progress = !cli.quiet && !cli.no_progress && !json_mode && config.output.progress;
    let allow_private = cli.allow_private_net || config.network.allow_private_network;

    match cli.command {
        Commands::Config(cfg) => handle_config(
            cfg.action,
            cli.config.as_deref(),
            &config,
            &paths,
            json_mode,
        ),
        Commands::Doctor => handle_doctor(&config, &paths, json_mode),
        Commands::Info(args) => handle_info(&config, args, allow_private, json_mode).await,
        Commands::Get(args) => {
            handle_get(
                &config,
                args,
                allow_private,
                show_progress,
                json_mode,
                cli.quiet,
            )
            .await
        }
        Commands::Playlist(args) => {
            handle_playlist(
                &config,
                args,
                allow_private,
                show_progress,
                json_mode,
                cli.quiet,
            )
            .await
        }
    }
}

async fn handle_info(
    config: &RuvidConfig,
    args: InfoArgs,
    allow_private: bool,
    json_mode: bool,
) -> RuviResult<()> {
    let headers = parse_headers(&args.headers)?;
    let ctx = ResolveContext::build(
        &config.network.user_agent,
        config.network.connect_timeout_seconds,
        args.timeout.unwrap_or(config.download.timeout_seconds),
        config.network.proxy.as_deref(),
        &headers,
        allow_private,
        config.download.retry_count,
    )?;

    let mut source = ResolverRegistry::resolve(&ctx, &args.url).await?;
    redact_media_source(&mut source);

    if json_mode {
        emit_json(&source);
    } else {
        render_source_info_human(&source, args.all_formats);
    }
    Ok(())
}

async fn handle_get(
    config: &RuvidConfig,
    args: GetArgs,
    allow_private: bool,
    show_progress: bool,
    json_mode: bool,
    quiet: bool,
) -> RuviResult<()> {
    let headers = parse_headers(&args.headers)?;
    let resume = if args.no_resume {
        false
    } else if args.resume {
        true
    } else {
        config.download.resume
    };

    let job = DownloadJob {
        url: args.url,
        output_template: args.output,
        format_spec: args.format,
        jobs: args.jobs.unwrap_or(config.download.jobs),
        resume,
        overwrite: args.overwrite || config.download.overwrite,
        limit_rate: args.limit_rate,
        retry_count: args.retry.unwrap_or(config.download.retry_count),
        timeout_seconds: args.timeout.unwrap_or(config.download.timeout_seconds),
        proxy: args.proxy,
        headers,
        write_manifest: args.write_manifest,
        to_stdout: args.stdout,
        subtitles: args.subtitles,
        subtitles_lang: args.subtitles_lang,
        allow_private_net: allow_private,
    };

    let outcome = execute_download(config, job, show_progress).await?;
    if !args.stdout {
        if json_mode {
            emit_json(&outcome);
        } else {
            render_download_human(&outcome, quiet);
        }
    }
    Ok(())
}

async fn handle_playlist(
    config: &RuvidConfig,
    args: PlaylistArgs,
    allow_private: bool,
    show_progress: bool,
    json_mode: bool,
    quiet: bool,
) -> RuviResult<()> {
    let ctx = ResolveContext::build(
        &config.network.user_agent,
        config.network.connect_timeout_seconds,
        config.download.timeout_seconds,
        config.network.proxy.as_deref(),
        &[],
        allow_private,
        config.download.retry_count,
    )?;

    let entries = fetch_playlist_entries(&ctx, &args.url).await?;
    let selected_indices = match args.items.as_deref() {
        Some(spec) => parse_item_selection(spec, entries.len())?,
        None => (1..=entries.len()).collect(),
    };

    let selected_entries: Vec<_> = entries
        .into_iter()
        .filter(|e| selected_indices.contains(&e.index))
        .collect();
    let total_selected = selected_entries.len();

    let base_dir = args
        .output
        .clone()
        .unwrap_or_else(|| config.download.output_dir.clone());
    std::fs::create_dir_all(&base_dir)?;

    let scheduler = DownloadScheduler::new(args.concurrency.max(1));
    let results = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let mut join_set = JoinSet::new();

    for entry in selected_entries {
        let permit = scheduler.acquire().await?;
        let cfg_c = config.clone();
        let base_dir_c = base_dir.clone();
        let format_c = args.format.clone();
        let jobs_c = args.jobs.unwrap_or(config.download.jobs);
        let overwrite_c = args.overwrite || config.download.overwrite;
        let skip_errors = args.skip_errors;
        let results_c = results.clone();
        let errors_c = errors.clone();

        join_set.spawn(async move {
            let _permit = permit;
            let title_part = entry
                .title
                .as_deref()
                .map(sanitize_filename)
                .unwrap_or_else(|| format!("item_{:03}", entry.index));
            let template = base_dir_c.join(format!("{:03}_{title_part}.%(ext)s", entry.index));
            let job = DownloadJob {
                url: entry.url.clone(),
                output_template: Some(template),
                format_spec: format_c,
                jobs: jobs_c,
                resume: cfg_c.download.resume,
                overwrite: overwrite_c,
                limit_rate: None,
                retry_count: cfg_c.download.retry_count,
                timeout_seconds: cfg_c.download.timeout_seconds,
                proxy: None,
                headers: Vec::new(),
                write_manifest: None,
                to_stdout: false,
                subtitles: false,
                subtitles_lang: None,
                allow_private_net: allow_private,
            };

            match execute_download(&cfg_c, job, show_progress).await {
                Ok(outcome) => {
                    results_c.lock().await.push(outcome);
                    Ok(())
                }
                Err(err) => {
                    if skip_errors {
                        errors_c.lock().await.push(PlaylistItemError {
                            index: entry.index,
                            url: redact_sensitive_url(&entry.url),
                            error: redact_urls_in_text(&err.to_string()),
                        });
                        Ok(())
                    } else {
                        Err(err)
                    }
                }
            }
        });
    }

    while let Some(res) = join_set.join_next().await {
        res.map_err(|err| RuviError::General(format!("playlist task panicked: {err}")))??;
    }

    let items = Arc::try_unwrap(results).unwrap().into_inner();
    let errs = Arc::try_unwrap(errors).unwrap().into_inner();
    let report = PlaylistReport {
        ok: errs.is_empty(),
        total_selected,
        succeeded: items.len(),
        failed: errs.len(),
        items,
        errors: errs,
    };

    if json_mode {
        emit_json(&report);
    } else {
        render_playlist_human(&report, quiet);
    }
    Ok(())
}

fn parse_headers(raw_headers: &[String]) -> RuviResult<Vec<(String, String)>> {
    let mut parsed = Vec::with_capacity(raw_headers.len());
    for h in raw_headers {
        let (k, v) = h.split_once(':').ok_or_else(|| {
            RuviError::InvalidArgument(format!(
                "invalid --header format '{h}' (expected KEY:VALUE)"
            ))
        })?;
        if k.trim().is_empty() {
            return Err(RuviError::InvalidArgument(
                "header name must not be empty".to_string(),
            ));
        }
        parsed.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(parsed)
}

fn handle_config(
    action: ConfigSubcommand,
    explicit: Option<&Path>,
    config: &RuvidConfig,
    paths: &AppPaths,
    json_mode: bool,
) -> RuviResult<()> {
    match action {
        ConfigSubcommand::Path => {
            if json_mode {
                #[derive(Serialize)]
                struct PathOut<'a> {
                    config_file: &'a Path,
                    cache_dir: &'a Path,
                    state_dir: &'a Path,
                }
                emit_json(&PathOut {
                    config_file: &paths.config_file,
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

fn handle_doctor(config: &RuvidConfig, paths: &AppPaths, json_mode: bool) -> RuviResult<()> {
    let out_dir = &config.download.output_dir;
    let writable = is_dir_or_ancestor_writable(out_dir);

    let report = DoctorReport {
        ok: writable,
        version: env!("CARGO_PKG_VERSION"),
        config_path: paths.config_file.to_string_lossy().into_owned(),
        config_exists: paths.config_file.exists(),
        cache_dir: paths.cache_dir.to_string_lossy().into_owned(),
        state_dir: paths.state_dir.to_string_lossy().into_owned(),
        output_dir: out_dir.to_string_lossy().into_owned(),
        output_dir_writable: writable,
    };

    if json_mode {
        emit_json(&report);
    } else {
        println!("ruvid version: {}", report.version);
        println!(
            "config: {} (exists={})",
            report.config_path, report.config_exists
        );
        println!("cache_dir: {}", report.cache_dir);
        println!("state_dir: {}", report.state_dir);
        println!(
            "output_dir: {} (writable={})",
            report.output_dir, report.output_dir_writable
        );
    }

    Ok(())
}

fn is_dir_or_ancestor_writable(path: &Path) -> bool {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };
    let mut cur = Some(abs.as_path());
    while let Some(p) = cur {
        if p.exists() {
            return p.is_dir()
                && std::fs::metadata(p)
                    .map(|m| !m.permissions().readonly())
                    .unwrap_or(false);
        }
        cur = p.parent();
    }
    false
}
