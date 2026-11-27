use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// User configuration, matching the Python version's JSON format exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub steam_api_key: String,
    pub steam_id: String,
    /// Optional IsThereAnyDeal API key; absent from the file when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub itad_api_key: Option<String>,
}

/// Returns `~/.config/bouncer/config.json`, falling back to
/// `./.config/bouncer/config.json` if `$HOME` is unset.
pub fn default_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("bouncer")
        .join("config.json")
}

/// Reads the config file at `path`. Returns `None` if the file is absent or malformed.
pub fn load_config(path: &Path) -> Option<Config> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Serializes `config` to pretty-printed JSON and writes it to `path`,
/// creating any missing parent directories first.
pub fn save_config(path: &Path, config: &Config) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config).map_err(io::Error::other)?;
    std::fs::write(path, json)
}
