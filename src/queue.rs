use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Schema version written into the queue file.
pub const QUEUE_VERSION: u32 = 1;

/// Per-game queue state, stored in the order that defines queue rank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    /// Normalized match key, produced by `normalize_key`.
    pub key: String,
    /// Display name as first seen, kept so the file is readable on its own.
    pub name: String,
    pub queued: bool,
    pub played: bool,
}

/// The full set of per-game queue state, persisted as one JSON file.
///
/// Entry order is the queue rank: a game's rank is its 1-based position among
/// the entries with `queued == true`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Queue {
    pub version: u32,
    pub entries: Vec<QueueEntry>,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            version: QUEUE_VERSION,
            entries: Vec::new(),
        }
    }
}

/// Queue flags for a single game.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameState {
    pub queued: bool,
    pub played: bool,
}

/// Normalizes a game name into the match key used to look up queue state.
///
/// Matches `loader::dedupe`'s key so state survives re-syncs and casing changes.
pub fn normalize_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Returns the default queue file path: `~/.local/share/backlog/queue.json`.
pub fn default_queue_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("backlog")
        .join("queue.json")
}

/// Reads the queue from disk, returning an empty queue if missing or malformed.
pub fn load(path: &Path) -> Queue {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

/// Writes the queue to disk, dropping entries whose flags are all cleared.
///
/// Written atomically (temp file plus rename) because, unlike a platform cache,
/// this file cannot be regenerated from an upstream source.
pub fn save(path: &Path, queue: &Queue) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pruned = Queue {
        version: QUEUE_VERSION,
        entries: queue
            .entries
            .iter()
            .filter(|e| e.queued || e.played)
            .cloned()
            .collect(),
    };
    let json = serde_json::to_string_pretty(&pruned).map_err(io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)
}

impl Queue {
    /// Returns the queue flags for `name`, defaulting to all-false when absent.
    pub fn state(&self, name: &str) -> GameState {
        let key = normalize_key(name);
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| GameState {
                queued: e.queued,
                played: e.played,
            })
            .unwrap_or_default()
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }

    /// Toggles the queued flag, appending newly queued games to the end of the
    /// queue so they take the last rank.
    pub fn toggle_queued(&mut self, name: &str) {
        let key = normalize_key(name);
        match self.index_of(&key) {
            Some(idx) if self.entries[idx].queued => self.entries[idx].queued = false,
            Some(idx) => {
                let mut entry = self.entries.remove(idx);
                entry.queued = true;
                self.entries.push(entry);
            }
            None => self.entries.push(QueueEntry {
                key,
                name: name.trim().to_string(),
                queued: true,
                played: false,
            }),
        }
    }

    /// Toggles the played flag, leaving queue membership and rank untouched.
    pub fn toggle_played(&mut self, name: &str) {
        let key = normalize_key(name);
        match self.index_of(&key) {
            Some(idx) => self.entries[idx].played = !self.entries[idx].played,
            None => self.entries.push(QueueEntry {
                key,
                name: name.trim().to_string(),
                queued: false,
                played: true,
            }),
        }
    }

    /// Returns the 1-based rank of a queued game, or `None` if it is not queued.
    pub fn rank(&self, name: &str) -> Option<usize> {
        let key = normalize_key(name);
        self.entries
            .iter()
            .filter(|e| e.queued)
            .position(|e| e.key == key)
            .map(|pos| pos + 1)
    }

    /// Returns the display names of queued games in rank order.
    pub fn queued_names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.queued)
            .map(|e| e.name.as_str())
            .collect()
    }

    /// Moves a queued game one rank later. Returns `false` if it is already last
    /// or is not queued.
    pub fn move_down(&mut self, name: &str) -> bool {
        self.swap_with_neighbor(name, true)
    }

    /// Moves a queued game one rank earlier. Returns `false` if it is already
    /// first or is not queued.
    pub fn move_up(&mut self, name: &str) -> bool {
        self.swap_with_neighbor(name, false)
    }

    /// Swaps a queued entry with the nearest queued entry after (or before) it.
    ///
    /// Played-only entries are skipped: they occupy positions in `entries` but
    /// hold no rank, so swapping across them would leave rank order unchanged.
    fn swap_with_neighbor(&mut self, name: &str, forward: bool) -> bool {
        let key = normalize_key(name);
        let Some(idx) = self.index_of(&key) else {
            return false;
        };
        if !self.entries[idx].queued {
            return false;
        }
        let neighbor = if forward {
            self.entries
                .iter()
                .enumerate()
                .skip(idx + 1)
                .find(|(_, e)| e.queued)
                .map(|(i, _)| i)
        } else {
            self.entries
                .iter()
                .enumerate()
                .take(idx)
                .rfind(|(_, e)| e.queued)
                .map(|(i, _)| i)
        };
        let Some(neighbor) = neighbor else {
            return false;
        };
        self.entries.swap(idx, neighbor);
        true
    }
}
