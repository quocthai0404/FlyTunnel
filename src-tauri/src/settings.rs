use crate::models::{TunnelSettings, APP_NAME};
use std::{env, fs, path::PathBuf};

const CONFIG_DIR_ENV: &str = "FLYTUNNEL_CONFIG_DIR";
const DATA_DIR_ENV: &str = "FLYTUNNEL_DATA_DIR";

pub fn load_settings() -> Result<TunnelSettings, String> {
    let path = settings_path()?;

    if !path.exists() {
        return Ok(TunnelSettings::default());
    }

    let raw = fs::read(&path).map_err(|error| format!("Failed to read settings: {error}"))?;
    serde_json::from_slice::<TunnelSettings>(&raw)
        .map(|settings| settings.sanitized())
        .map_err(|error| format!("Failed to parse settings: {error}"))
}

pub fn save_settings(settings: &TunnelSettings) -> Result<TunnelSettings, String> {
    let sanitized = settings.sanitized();
    let path = settings_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create settings directory: {error}"))?;
    }

    let serialized = serde_json::to_vec_pretty(&sanitized)
        .map_err(|error| format!("Failed to serialize settings: {error}"))?;
    fs::write(&path, serialized).map_err(|error| format!("Failed to save settings: {error}"))?;

    Ok(sanitized)
}

pub fn settings_path() -> Result<PathBuf, String> {
    Ok(config_root()?.join("settings.json"))
}

pub fn runtime_root() -> Result<PathBuf, String> {
    let path = data_root()?.join("runtime");
    fs::create_dir_all(&path)
        .map_err(|error| format!("Failed to create runtime directory: {error}"))?;
    Ok(path)
}

pub fn bin_root() -> Result<PathBuf, String> {
    let path = data_root()?.join("bin");
    fs::create_dir_all(&path)
        .map_err(|error| format!("Failed to create bin directory: {error}"))?;
    Ok(path)
}

fn config_root() -> Result<PathBuf, String> {
    let path = root_from_env(CONFIG_DIR_ENV)
        .or_else(|| dirs::config_dir().or_else(dirs::home_dir))
        .unwrap_or(
            env::current_dir()
                .map_err(|error| format!("Failed to resolve config directory: {error}"))?,
        )
        .join(APP_NAME);

    fs::create_dir_all(&path)
        .map_err(|error| format!("Failed to create config directory: {error}"))?;
    Ok(path)
}

fn data_root() -> Result<PathBuf, String> {
    let path = root_from_env(DATA_DIR_ENV)
        .or_else(|| {
            dirs::data_local_dir()
                .or_else(dirs::data_dir)
                .or_else(dirs::home_dir)
        })
        .unwrap_or(
            env::current_dir()
                .map_err(|error| format!("Failed to resolve data directory: {error}"))?,
        )
        .join(APP_NAME);

    fs::create_dir_all(&path)
        .map_err(|error| format!("Failed to create data directory: {error}"))?;
    Ok(path)
}

fn root_from_env(name: &str) -> Option<PathBuf> {
    env::var_os(name).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}
