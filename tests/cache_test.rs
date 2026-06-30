use backlog::{Game, cache};
use tempfile::TempDir;

#[test]
fn test_read_cache_returns_none_when_missing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nonexistent.json");
    assert!(cache::read_cache(&path).is_none());
}

#[test]
fn test_write_and_read_cache() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.json");
    let games = vec![
        Game {
            name: "Hollow Knight".to_string(),
            platform: "gog".to_string(),
        },
        Game {
            name: "Celeste".to_string(),
            platform: "epic".to_string(),
        },
    ];
    cache::write_cache(&path, &games, "test_source").unwrap();
    let data = cache::read_cache(&path).unwrap();
    assert_eq!(data.games.len(), 2);
    assert_eq!(data.games[0].name, "Hollow Knight");
    assert_eq!(data.source, "test_source");
    assert!(!data.last_updated.is_empty());
}

#[test]
fn test_write_cache_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("cache.json");
    let games = vec![Game {
        name: "Test".to_string(),
        platform: "steam".to_string(),
    }];
    cache::write_cache(&path, &games, "test").unwrap();
    assert!(path.exists());
}

#[test]
fn test_is_stale_with_old_timestamp() {
    assert!(cache::is_stale("2020-01-01T00:00:00+00:00", 7));
}

#[test]
fn test_is_stale_with_fresh_timestamp() {
    let now = chrono::Utc::now().to_rfc3339();
    assert!(!cache::is_stale(&now, 7));
}

#[test]
fn test_cache_format_matches_python() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.json");
    let games = vec![Game {
        name: "Half-Life".to_string(),
        platform: "steam".to_string(),
    }];
    cache::write_cache(&path, &games, "steam_api").unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw["last_updated"].is_string());
    assert_eq!(raw["source"], "steam_api");
    assert_eq!(raw["games"][0]["name"], "Half-Life");
    assert_eq!(raw["games"][0]["platform"], "steam");
}
