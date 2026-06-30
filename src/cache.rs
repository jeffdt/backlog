use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

use crate::Game;

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheFile {
    pub last_updated: String,
    pub source: String,
    pub games: Vec<Game>,
}

/// Returns the default cache directory: `~/.cache/backlog`.
pub fn default_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("backlog")
}

/// Reads a cache file from disk, returning `None` if missing or malformed.
pub fn read_cache(path: &Path) -> Option<CacheFile> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Writes games to a JSON cache file, creating parent directories as needed.
pub fn write_cache(path: &Path, games: &[Game], source: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = CacheFile {
        last_updated: Utc::now().to_rfc3339(),
        source: source.to_string(),
        games: games.to_vec(),
    };
    let json = serde_json::to_string_pretty(&data).map_err(io::Error::other)?;
    std::fs::write(path, json)
}

/// Returns `true` if the cache timestamp is older than `max_age_days`.
/// Also returns `true` if the timestamp cannot be parsed.
pub fn is_stale(last_updated: &str, max_age_days: i64) -> bool {
    let Ok(updated) = DateTime::parse_from_rfc3339(last_updated) else {
        return true;
    };
    let age = Utc::now() - updated.with_timezone(&Utc);
    age > chrono::Duration::days(max_age_days)
}
