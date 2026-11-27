pub mod cache;
pub mod config;
pub mod itad;
pub mod loader;
pub mod output;
pub mod queue;
pub mod search;
pub mod sources;
pub mod sync;
pub mod tui;

use serde::{Deserialize, Serialize};

/// A game entry from any supported platform.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Game {
    pub name: String,
    pub platform: String,
    /// RFC3339 time sync first saw this title in the store, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<String>,
}

/// A deduplicated library entry combining all platforms a game is owned on.
#[derive(Debug, Clone, Default)]
pub struct LibraryEntry {
    /// Display name using the first-seen casing and spacing.
    pub name: String,
    /// Platforms the game is owned on, in load order (epic, gog, amazon, steam).
    pub platforms: Vec<String>,
    /// RFC3339 first-seen time per platform; only platforms with a known date appear.
    pub first_seen: std::collections::BTreeMap<String, String>,
}
