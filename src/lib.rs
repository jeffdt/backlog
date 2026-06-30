pub mod cache;
pub mod config;
pub mod loader;
pub mod search;
pub mod sources;
pub mod sync;
pub mod tui;

use serde::{Deserialize, Serialize};

/// A game entry from any supported platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub name: String,
    pub platform: String,
}

/// A deduplicated library entry combining all platforms a game is owned on.
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    /// Display name using the first-seen casing and spacing.
    pub name: String,
    /// Platforms the game is owned on, in load order (epic, gog, amazon, steam).
    pub platforms: Vec<String>,
}
