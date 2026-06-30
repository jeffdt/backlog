use backlog::{cache, sync};
use std::fs;
use tempfile::TempDir;

fn setup_heroic_dir(dir: &TempDir) {
    fs::write(
        dir.path().join("legendary_library.json"),
        r#"{"library": [{"title": "Celeste", "app_name": "abc"}]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("gog_library.json"),
        r#"{"games": [
            {"title": "Hollow Knight", "app_name": "hk"},
            {"title": "Redist", "app_name": "gog-redist"}
        ]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("nile_library.json"),
        r#"{"library": [{"title": "The Dig", "app_name": "amzn1"}]}"#,
    )
    .unwrap();
}

#[test]
fn test_sync_heroic_writes_caches() {
    let heroic_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    setup_heroic_dir(&heroic_dir);

    let reports = sync::sync_heroic(heroic_dir.path(), cache_dir.path());
    assert_eq!(reports.len(), 3);

    let epic_cache = cache::read_cache(&cache_dir.path().join("epic.json")).unwrap();
    assert_eq!(epic_cache.games.len(), 1);
    assert_eq!(epic_cache.games[0].name, "Celeste");

    let gog_cache = cache::read_cache(&cache_dir.path().join("gog.json")).unwrap();
    assert_eq!(gog_cache.games.len(), 1);
    assert_eq!(gog_cache.games[0].name, "Hollow Knight");

    let amazon_cache = cache::read_cache(&cache_dir.path().join("amazon.json")).unwrap();
    assert_eq!(amazon_cache.games.len(), 1);
}

#[test]
fn test_sync_steam_skips_when_no_config() {
    let cache_dir = TempDir::new().unwrap();
    let config_path = cache_dir.path().join("nonexistent.json");

    let report = sync::sync_steam(&config_path, cache_dir.path());
    assert!(matches!(report.status, sync::SyncStatus::Skipped(_)));
}

#[test]
fn test_sync_heroic_handles_missing_files() {
    let heroic_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    // Empty heroic dir, no library files
    let reports = sync::sync_heroic(heroic_dir.path(), cache_dir.path());
    assert_eq!(reports.len(), 3);
    // No cache files should have been written
    assert!(!cache_dir.path().join("epic.json").exists());
}

#[test]
fn test_sync_all_returns_four_reports() {
    let heroic_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let config_path = cache_dir.path().join("nonexistent.json");
    setup_heroic_dir(&heroic_dir);

    let reports = sync::sync_all(heroic_dir.path(), &config_path, cache_dir.path());
    // 3 heroic + 1 steam
    assert_eq!(reports.len(), 4);
}

#[test]
fn test_sync_heroic_all_ok_status() {
    let heroic_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    setup_heroic_dir(&heroic_dir);

    let reports = sync::sync_heroic(heroic_dir.path(), cache_dir.path());
    for report in &reports {
        assert!(matches!(report.status, sync::SyncStatus::Ok));
    }
}

#[test]
fn test_sync_heroic_reports_platform_names() {
    let heroic_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    setup_heroic_dir(&heroic_dir);

    let reports = sync::sync_heroic(heroic_dir.path(), cache_dir.path());
    let platforms: Vec<&str> = reports.iter().map(|r| r.platform.as_str()).collect();
    assert!(platforms.contains(&"epic"));
    assert!(platforms.contains(&"gog"));
    assert!(platforms.contains(&"amazon"));
}
