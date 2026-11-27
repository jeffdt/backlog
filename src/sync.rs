use std::path::Path;

use crate::Game;
use crate::cache;
use crate::config;
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
pub fn sync_heroic(heroic_dir: &Path, cache_dir: &Path) -> Vec<SyncReport> {
    let sources: Vec<HeroicSource> = vec![
        ("epic", heroic::load_epic, "legendary_library.json"),
        ("gog", heroic::load_gog, "gog_library.json"),
        ("amazon", heroic::load_amazon, "nile_library.json"),
    ];

    sources
        .into_iter()
        .map(|(platform, loader, filename)| {
            let source_path = heroic_dir.join(filename);
            let games = loader(&source_path);
            let cache_path = cache_dir.join(format!("{platform}.json"));
            if games.is_empty() && !source_path.exists() {
                return SyncReport {
                    platform: platform.to_string(),
                    game_count: 0,
                    status: skip_and_remove_cache(&cache_path, format!("{filename} not found")),
                };
            }
            match cache::write_cache(&cache_path, &games, "heroic_cache") {
                Ok(()) => SyncReport {
                    platform: platform.to_string(),
                    game_count: games.len(),
                    status: SyncStatus::Ok,
                },
                Err(e) => SyncReport {
                    platform: platform.to_string(),
                    game_count: 0,
                    status: SyncStatus::Error(e.to_string()),
                },
            }
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
        return SyncReport {
            platform: "steam".to_string(),
            game_count: 0,
            status: skip_and_remove_cache(
                &cache_dir.join("steam.json"),
                "no config found, run `bouncer setup` first".to_string(),
            ),
        };
    };
    match steam::fetch_steam_library(&cfg.steam_api_key, &cfg.steam_id) {
        Ok(games) => {
            let count = games.len();
            match cache::write_cache(&cache_dir.join("steam.json"), &games, "steam_api") {
                Ok(()) => SyncReport {
                    platform: "steam".to_string(),
                    game_count: count,
                    status: SyncStatus::Ok,
                },
                Err(e) => SyncReport {
                    platform: "steam".to_string(),
                    game_count: 0,
                    status: SyncStatus::Error(e.to_string()),
                },
            }
        }
        Err(e) => SyncReport {
            platform: "steam".to_string(),
            game_count: 0,
            status: SyncStatus::Error(e),
        },
    }
}

/// Syncs all sources: Heroic (Epic, GOG, Amazon) followed by Steam.
pub fn sync_all(heroic_dir: &Path, config_path: &Path, cache_dir: &Path) -> Vec<SyncReport> {
    let mut reports = sync_heroic(heroic_dir, cache_dir);
    reports.push(sync_steam(config_path, cache_dir));
    reports
}
