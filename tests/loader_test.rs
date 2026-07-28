use backlog::{Game, cache, loader};
use tempfile::TempDir;

fn setup_caches(dir: &TempDir) {
    let games_steam = vec![
        Game {
            name: "Half-Life".to_string(),
            platform: "steam".to_string(),
        },
        Game {
            name: "Portal".to_string(),
            platform: "steam".to_string(),
        },
    ];
    let games_epic = vec![Game {
        name: "Celeste".to_string(),
        platform: "epic".to_string(),
    }];
    cache::write_cache(&dir.path().join("steam.json"), &games_steam, "steam_api").unwrap();
    cache::write_cache(&dir.path().join("epic.json"), &games_epic, "heroic_cache").unwrap();
}

#[test]
fn test_load_all_games_merges_platforms() {
    let dir = TempDir::new().unwrap();
    setup_caches(&dir);
    let result = loader::load_all_games(dir.path());
    assert_eq!(result.games.len(), 3);
    assert!(result.warnings.is_empty());
    assert!(result.oldest_update.is_some());
}

#[test]
fn test_dedupe_collapses_cross_store_duplicates() {
    let games = vec![
        Game {
            name: "Card Shark".to_string(),
            platform: "epic".to_string(),
        },
        Game {
            name: "Card Shark".to_string(),
            platform: "steam".to_string(),
        },
    ];
    let entries = loader::dedupe(games);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Card Shark");
    assert_eq!(entries[0].platforms, vec!["epic", "steam"]);
}

#[test]
fn test_dedupe_keeps_distinct_games_separate() {
    let games = vec![
        Game {
            name: "Hades".to_string(),
            platform: "epic".to_string(),
        },
        Game {
            name: "Celeste".to_string(),
            platform: "epic".to_string(),
        },
    ];
    let entries = loader::dedupe(games);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_dedupe_folds_case_and_trim_but_keeps_first_seen_display() {
    let games = vec![
        Game {
            name: "Card Shark".to_string(),
            platform: "epic".to_string(),
        },
        Game {
            name: "  card shark  ".to_string(),
            platform: "steam".to_string(),
        },
    ];
    let entries = loader::dedupe(games);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Card Shark");
    assert_eq!(entries[0].platforms, vec!["epic", "steam"]);
}

#[test]
fn test_dedupe_collapses_intra_platform_duplicates() {
    let games = vec![
        Game {
            name: "Portal".to_string(),
            platform: "steam".to_string(),
        },
        Game {
            name: "Portal".to_string(),
            platform: "steam".to_string(),
        },
    ];
    let entries = loader::dedupe(games);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].platforms, vec!["steam"]);
}

#[test]
fn test_dedupe_preserves_first_seen_order() {
    let games = vec![
        Game {
            name: "Zelda".to_string(),
            platform: "steam".to_string(),
        },
        Game {
            name: "Abzu".to_string(),
            platform: "epic".to_string(),
        },
    ];
    let entries = loader::dedupe(games);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Zelda", "Abzu"]);
}

#[test]
fn test_load_all_games_warns_on_stale() {
    let dir = TempDir::new().unwrap();
    let stale_json = r#"{
        "last_updated": "2020-01-01T00:00:00+00:00",
        "source": "steam_api",
        "games": [{"name": "Old Game", "platform": "steam"}]
    }"#;
    std::fs::write(dir.path().join("steam.json"), stale_json).unwrap();
    let result = loader::load_all_games(dir.path());
    assert_eq!(result.games.len(), 1);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("steam"));
}

#[test]
fn test_load_all_games_empty_dir() {
    let dir = TempDir::new().unwrap();
    let result = loader::load_all_games(dir.path());
    assert!(result.games.is_empty());
    assert!(result.warnings.is_empty());
    assert!(result.oldest_update.is_none());
}

#[test]
fn test_format_sync_age_none() {
    assert_eq!(loader::format_sync_age(None), "never synced");
}

#[test]
fn test_format_sync_age_recent() {
    let recent = chrono::Utc::now() - chrono::Duration::seconds(30);
    assert_eq!(loader::format_sync_age(Some(recent)), "synced just now");
}

#[test]
fn test_format_sync_age_hours() {
    let hours_ago = chrono::Utc::now() - chrono::Duration::hours(3);
    assert_eq!(loader::format_sync_age(Some(hours_ago)), "synced 3h ago");
}

#[test]
fn test_format_sync_age_days() {
    let days_ago = chrono::Utc::now() - chrono::Duration::days(5);
    assert_eq!(loader::format_sync_age(Some(days_ago)), "synced 5d ago");
}

#[test]
fn dedupe_merges_exactly_the_names_queue_normalize_key_agrees_on() {
    // Queue state is addressed by `queue::normalize_key`, so every library
    // entry `dedupe` produces must correspond to exactly one queue key. If the
    // two ever diverge, previously queued and played games become unreachable.
    let names = [
        "Tunic",
        "  tunic",
        "TUNIC ",
        "Outer Wilds",
        "outer  wilds",
        "Café",
    ];
    for a in names {
        for b in names {
            let entries = loader::dedupe(vec![
                Game {
                    name: a.to_string(),
                    platform: "steam".to_string(),
                },
                Game {
                    name: b.to_string(),
                    platform: "epic".to_string(),
                },
            ]);
            let merged = entries.len() == 1;
            let same_key = backlog::queue::normalize_key(a) == backlog::queue::normalize_key(b);
            assert_eq!(
                merged, same_key,
                "dedupe and normalize_key disagree on {a:?} vs {b:?}"
            );
        }
    }
}
