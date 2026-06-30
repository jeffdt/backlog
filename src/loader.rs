use chrono::{DateTime, Utc};
use std::path::Path;

use crate::Game;
use crate::cache;

const PLATFORMS: &[&str] = &["epic", "gog", "amazon", "steam"];

pub struct LoadResult {
    pub games: Vec<Game>,
    pub warnings: Vec<String>,
    pub oldest_update: Option<DateTime<Utc>>,
}

/// Reads all platform caches from `cache_dir` and merges them into a single list.
///
/// Platforms without a cache file are silently skipped. Platforms with a stale
/// cache (older than 7 days) produce a warning entry.
pub fn load_all_games(cache_dir: &Path) -> LoadResult {
    let mut games = Vec::new();
    let mut warnings = Vec::new();
    let mut oldest_update: Option<DateTime<Utc>> = None;

    for platform in PLATFORMS {
        let cache_file = cache_dir.join(format!("{platform}.json"));
        let Some(data) = cache::read_cache(&cache_file) else {
            continue;
        };

        if let Ok(updated) = DateTime::parse_from_rfc3339(&data.last_updated) {
            let updated_utc = updated.with_timezone(&Utc);
            if oldest_update.is_none() || Some(updated_utc) < oldest_update {
                oldest_update = Some(updated_utc);
            }
        }

        if cache::is_stale(&data.last_updated, 7) {
            let date = data.last_updated.get(..10).unwrap_or(&data.last_updated);
            warnings.push(format!(
                "{platform} cache is stale (last updated: {date}). Run `backlog sync` to refresh."
            ));
        }

        games.extend(data.games);
    }

    LoadResult { games, warnings, oldest_update }
}

/// Formats how long ago the library was last synced into a human-readable string.
pub fn format_sync_age(synced_at: Option<DateTime<Utc>>) -> String {
    let Some(synced_at) = synced_at else {
        return "never synced".to_string();
    };
    let age = Utc::now() - synced_at;
    let days = age.num_days();
    if days > 0 {
        return format!("synced {days}d ago");
    }
    let hours = age.num_hours();
    if hours > 0 {
        return format!("synced {hours}h ago");
    }
    let minutes = age.num_minutes();
    if minutes > 0 {
        return format!("synced {minutes}m ago");
    }
    "synced just now".to_string()
}
