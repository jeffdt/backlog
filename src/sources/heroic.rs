use std::path::{Path, PathBuf};

use crate::Game;

/// Returns the path to Heroic's store_cache directory on macOS.
pub fn heroic_store_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("heroic")
        .join("store_cache")
}

/// Loads Epic Games titles from a Heroic `legendary_library.json` file.
pub fn load_epic(path: &Path) -> Vec<Game> {
    load_heroic_library(path, "library", "epic")
}

/// Loads GOG titles from a Heroic `gog_library.json` file, filtering out the
/// `gog-redist` redistributables entry.
pub fn load_gog(path: &Path) -> Vec<Game> {
    let Some(data) = read_json(path) else {
        return Vec::new();
    };
    let Some(games) = data.get("games").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    games
        .iter()
        .filter(|g| g.get("app_name").and_then(|v| v.as_str()) != Some("gog-redist"))
        .filter_map(|g| {
            let name = g.get("title")?.as_str()?;
            Some(Game {
                name: name.to_string(),
                platform: "gog".to_string(),
            })
        })
        .collect()
}

/// Loads Amazon Games titles from a Heroic `nile_library.json` file.
pub fn load_amazon(path: &Path) -> Vec<Game> {
    load_heroic_library(path, "library", "amazon")
}

fn load_heroic_library(path: &Path, key: &str, platform: &str) -> Vec<Game> {
    let Some(data) = read_json(path) else {
        return Vec::new();
    };
    let Some(library) = data.get(key).and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    library
        .iter()
        .filter_map(|g| {
            let name = g.get("title")?.as_str()?;
            Some(Game {
                name: name.to_string(),
                platform: platform.to_string(),
            })
        })
        .collect()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}
