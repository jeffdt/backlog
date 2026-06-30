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
