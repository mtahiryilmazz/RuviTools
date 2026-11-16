use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::config::schema::RuvidConfig;
use crate::error::{RuviError, RuviResult};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
}

pub fn resolve_paths(explicit_config: Option<&Path>) -> RuviResult<AppPaths> {
    let base_dirs = BaseDirs::new();

    let config_root = if let Ok(dir) = std::env::var("RUVI_CONFIG_DIR") {
        PathBuf::from(dir)
    } else if let Some(ref base) = base_dirs {
        base.config_dir().join("ruvi")
    } else {
        PathBuf::from(".ruvi").join("config")
    };

    let cache_root = if let Ok(dir) = std::env::var("RUVI_CACHE_DIR") {
        PathBuf::from(dir)
    } else if let Some(ref base) = base_dirs {
        base.cache_dir().join("ruvi")
    } else {
        PathBuf::from(".ruvi").join("cache")
    };

    let state_root = if let Ok(dir) = std::env::var("RUVI_STATE_DIR") {
        PathBuf::from(dir)
    } else if let Some(ref base) = base_dirs {
        base.data_local_dir().join("ruvi")
    } else {
        PathBuf::from(".ruvi").join("state")
    };

    let config_file = match explicit_config {
        Some(path) => path.to_path_buf(),
        None => config_root.join("ruvid.toml"),
    };

    Ok(AppPaths {
        config_file,
        cache_dir: cache_root.join("ruvid"),
        state_dir: state_root.join("ruvid"),
    })
}

pub fn load_config(path: Option<&Path>) -> RuviResult<(RuvidConfig, AppPaths)> {
    let paths = resolve_paths(path)?;
    if let Some(explicit) = path {
        if !explicit.exists() {
            return Err(RuviError::Config(format!(
                "config file not found: {}",
                explicit.display()
            )));
        }
    } else if !paths.config_file.exists() {
        return Ok((RuvidConfig::default(), paths));
    }

    let raw = std::fs::read_to_string(&paths.config_file).map_err(|err| {
        RuviError::Config(format!(
            "failed to read config {}: {err}",
            paths.config_file.display()
        ))
    })?;
    let parsed: RuvidConfig = toml::from_str(&raw).map_err(|err| {
        RuviError::Config(format!(
            "invalid TOML in config {}: {err}",
            paths.config_file.display()
        ))
    })?;
    validate_config(&parsed)?;
    Ok((parsed, paths))
}

pub fn validate_config(cfg: &RuvidConfig) -> RuviResult<()> {
    if cfg.download.jobs == 0 || cfg.download.jobs > 32 {
        return Err(RuviError::Config(
            "download.jobs must be between 1 and 32".to_string(),
        ));
    }
    if cfg.download.timeout_seconds == 0 {
        return Err(RuviError::Config(
            "download.timeout_seconds must be greater than 0".to_string(),
        ));
    }
    if cfg.network.connect_timeout_seconds == 0 {
        return Err(RuviError::Config(
            "network.connect_timeout_seconds must be greater than 0".to_string(),
        ));
    }
    let color = cfg.output.color.to_ascii_lowercase();
    if !matches!(color.as_str(), "auto" | "always" | "never") {
        return Err(RuviError::Config(format!(
            "output.color must be auto|always|never, got '{}'",
            cfg.output.color
        )));
    }
    if let Err(err) = crate::downloader::throttle::parse_rate_limit(&cfg.network.limit_rate) {
        return Err(RuviError::Config(format!(
            "invalid network.limit_rate: {err}"
        )));
    }
    Ok(())
}

pub fn default_config_toml() -> RuviResult<String> {
    toml::to_string_pretty(&RuvidConfig::default())
        .map_err(|err| RuviError::Config(format!("failed to serialize default config: {err}")))
}

pub fn init_config(path: Option<&Path>, overwrite: bool) -> RuviResult<PathBuf> {
    let paths = resolve_paths(path)?;
    if paths.config_file.exists() && !overwrite {
        return Err(RuviError::Filesystem(format!(
            "config file already exists: {} (pass --overwrite to replace)",
            paths.config_file.display()
        )));
    }
    if let Some(parent) = paths.config_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = default_config_toml()?;
    std::fs::write(&paths.config_file, body)?;
    Ok(paths.config_file)
}
