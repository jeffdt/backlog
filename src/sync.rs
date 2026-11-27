use std::collections::{BTreeSet, HashMap};

use chrono::Utc;
use std::path::Path;

use crate::Game;
use crate::cache;
use crate::config;
use crate::queue;
use crate::sources::heroic;
use crate::sources::steam;

type HeroicSource = (&'static str, fn(&Path) -> Vec<Game>, &'static str);

/// Status of a single sync operation.
pub enum SyncStatus {
    Ok,
    Skipped(String),
    Error(String),
}

/// Result of syncing one platform's game library.
pub struct SyncReport {
    pub platform: String,
    pub game_count: usize,
    pub status: SyncStatus,
    /// Titles in the new library that were not in the previous cache, sorted.
    pub added: Vec<String>,
    /// Titles in the previous cache that are no longer in the library, sorted.
    pub removed: Vec<String>,
}

impl SyncReport {
    pub(crate) fn without_games(platform: &str, status: SyncStatus) -> Self {
        SyncReport {
            platform: platform.to_string(),
            game_count: 0,
            status,
            added: Vec::new(),
            removed: Vec::new(),
        }
    }
}

/// Writes `games` to `cache_path`, reporting which titles were added or
/// removed relative to the cache it replaces. A failed write keeps the old
/// cache and reports no changes.
fn write_and_report(platform: &str, cache_path: &Path, games: &[Game], source: &str) -> SyncReport {
    let old_cache = cache::read_cache(cache_path);
    let old_names: BTreeSet<String> = old_cache
        .iter()
        .flat_map(|c| c.games.iter().map(|g| g.name.clone()))
        .collect();
    // An empty old cache says nothing about when titles were first seen.
    let old: Option<HashMap<String, Option<String>>> =
        old_cache.filter(|c| !c.games.is_empty()).map(|c| {
            c.games
                .into_iter()
                .map(|g| (queue::normalize_key(&g.name), g.first_seen))
                .collect()
        });
    let now = Utc::now().to_rfc3339();
    let stamped: Vec<Game> = games
        .iter()
        .map(|g| Game {
            first_seen: match &old {
                Some(m) => m
                    .get(&queue::normalize_key(&g.name))
                    .cloned()
                    .unwrap_or_else(|| Some(now.clone())),
                None => None,
            },
            ..g.clone()
        })
        .collect();
    if let Err(e) = cache::write_cache(cache_path, &stamped, source) {
        return SyncReport::without_games(platform, SyncStatus::Error(e.to_string()));
    }
    let new_names: BTreeSet<String> = games.iter().map(|g| g.name.clone()).collect();
    SyncReport {
        platform: platform.to_string(),
        game_count: games.len(),
        status: SyncStatus::Ok,
        added: new_names.difference(&old_names).cloned().collect(),
        removed: old_names.difference(&new_names).cloned().collect(),
    }
}

/// Removes the cache at `cache_path` for a source that no longer exists, so its
/// games stop appearing in results, and appends a note to `reason` when a file
/// was actually removed.
fn skip_and_remove_cache(cache_path: &Path, reason: String) -> SyncStatus {
    match std::fs::remove_file(cache_path) {
        Ok(()) => SyncStatus::Skipped(format!("{reason}, removed cached library")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SyncStatus::Skipped(reason),
        Err(e) => SyncStatus::Error(format!(
            "{reason}, but could not remove cached library: {e}"
        )),
    }
}

/// Syncs all three Heroic-managed stores (Epic, GOG, Amazon) from `heroic_dir`,
/// writing each to a separate cache file in `cache_dir`.
///
/// A source is reported as `Skipped` only when its library file does not exist
/// and the loader returns no games; its existing cache is then removed. If the
/// file exists but is empty, the cache is still written (with zero entries).
///
/// Amazon reads Nile's own library (beside `heroic_dir`) first and falls back
/// to Heroic's `nile_library.json` only when Nile's file yields no games.
pub fn sync_heroic(heroic_dir: &Path, cache_dir: &Path) -> Vec<SyncReport> {
    let nile_games = heroic::load_nile(&heroic::nile_library_path(heroic_dir));
    let sources: Vec<HeroicSource> = vec![
        ("epic", heroic::load_epic, "legendary_library.json"),
        ("gog", heroic::load_gog, "gog_library.json"),
        ("amazon", heroic::load_amazon, "nile_library.json"),
    ];

    sources
        .into_iter()
        .map(|(platform, loader, filename)| {
            let cache_path = cache_dir.join(format!("{platform}.json"));
            if platform == "amazon" && !nile_games.is_empty() {
                return write_and_report(platform, &cache_path, &nile_games, "nile");
            }
            let source_path = heroic_dir.join(filename);
            let games = loader(&source_path);
            if games.is_empty() && !source_path.exists() {
                return SyncReport::without_games(
                    platform,
                    skip_and_remove_cache(&cache_path, format!("{filename} not found")),
                );
            }
            write_and_report(platform, &cache_path, &games, "heroic_cache")
        })
        .collect()
}

/// Syncs the Steam library using credentials from `config_path`.
///
/// Returns `Skipped` and removes the Steam cache if no config file exists.
/// Returns `Error`, keeping the existing cache, if the API call or cache write
/// fails.
pub fn sync_steam(config_path: &Path, cache_dir: &Path) -> SyncReport {
    let Some(cfg) = config::load_config(config_path) else {
        return SyncReport::without_games(
            "steam",
            skip_and_remove_cache(
                &cache_dir.join("steam.json"),
                "no config found, run `bouncer setup` first".to_string(),
            ),
        );
    };
    match steam::fetch_steam_library(&cfg.steam_api_key, &cfg.steam_id) {
        Ok(games) => write_and_report("steam", &cache_dir.join("steam.json"), &games, "steam_api"),
        Err(e) => SyncReport::without_games("steam", SyncStatus::Error(e)),
    }
}

/// Syncs all sources: Heroic (Epic, GOG, Amazon) followed by Steam.
pub fn sync_all(heroic_dir: &Path, config_path: &Path, cache_dir: &Path) -> Vec<SyncReport> {
    let mut reports = sync_heroic(heroic_dir, cache_dir);
    reports.push(sync_steam(config_path, cache_dir));
    reports
}
