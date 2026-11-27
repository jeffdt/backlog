use bouncer::{cache, sync};
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

#[test]
fn test_sync_removes_cache_only_for_skipped_sources() {
    struct Case {
        platform: &'static str,
        source_file: Option<&'static str>,
        cache_survives: bool,
        removal_noted: bool,
    }
    const CASES: &[Case] = &[
        Case {
            platform: "epic",
            source_file: None,
            cache_survives: false,
            removal_noted: true,
        },
        Case {
            platform: "gog",
            source_file: None,
            cache_survives: false,
            removal_noted: true,
        },
        Case {
            platform: "amazon",
            source_file: None,
            cache_survives: false,
            removal_noted: true,
        },
        Case {
            platform: "steam",
            source_file: None,
            cache_survives: false,
            removal_noted: true,
        },
        Case {
            platform: "epic",
            source_file: Some(r#"{"library": [{"title": "Celeste", "app_name": "abc"}]}"#),
            cache_survives: true,
            removal_noted: false,
        },
    ];

    let mut misses = Vec::new();
    for case in CASES {
        let heroic_dir = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let cache_path = cache_dir.path().join(format!("{}.json", case.platform));
        cache::write_cache(&cache_path, &[], "old").unwrap();
        if let Some(contents) = case.source_file {
            fs::write(heroic_dir.path().join("legendary_library.json"), contents).unwrap();
        }
        let config_path = cache_dir.path().join("nonexistent.json");

        let reports = sync::sync_all(heroic_dir.path(), &config_path, cache_dir.path());
        let report = reports
            .iter()
            .find(|r| r.platform == case.platform)
            .unwrap();
        let noted = matches!(&report.status, sync::SyncStatus::Skipped(r) if r.contains("removed cached library"));
        let survives = cache_path.exists();
        if survives != case.cache_survives || noted != case.removal_noted {
            misses.push(format!(
                "{} (source {:?}): cache_survives={survives} removal_noted={noted}",
                case.platform,
                case.source_file.is_some()
            ));
        }
    }
    assert!(misses.is_empty(), "misses:\n{}", misses.join("\n"));
}

#[test]
fn test_sync_skip_without_existing_cache_does_not_note_removal() {
    let heroic_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let config_path = cache_dir.path().join("nonexistent.json");

    for report in sync::sync_all(heroic_dir.path(), &config_path, cache_dir.path()) {
        assert!(
            matches!(&report.status, sync::SyncStatus::Skipped(r) if !r.contains("removed")),
            "{} should be a plain skip",
            report.platform
        );
    }
}

#[test]
fn test_sync_heroic_reports_added_and_removed_titles() {
    // (previous epic cache or None, new library titles, expected added, expected removed)
    type Row = (
        Option<&'static [&'static str]>,
        &'static [&'static str],
        &'static [&'static str],
        &'static [&'static str],
    );
    const CASES: &[Row] = &[
        (None, &["Celeste", "Hades"], &["Celeste", "Hades"], &[]),
        (Some(&["Celeste"]), &["Celeste"], &[], &[]),
        (
            Some(&["Celeste"]),
            &["Celeste", "Ōkami HD"],
            &["Ōkami HD"],
            &[],
        ),
        (
            Some(&["Celeste", "Control™"]),
            &["Celeste"],
            &[],
            &["Control™"],
        ),
        (Some(&["Hades"]), &["Hades II"], &["Hades II"], &["Hades"]),
        (Some(&["Celeste"]), &[], &[], &["Celeste"]),
        (Some(&["Celeste", "Celeste"]), &["Celeste"], &[], &[]),
    ];

    let mut misses = Vec::new();
    for (old, new, want_added, want_removed) in CASES {
        let heroic_dir = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();
        let library: Vec<String> = new
            .iter()
            .map(|t| format!(r#"{{"title": "{t}", "app_name": "x"}}"#))
            .collect();
        fs::write(
            heroic_dir.path().join("legendary_library.json"),
            format!(r#"{{"library": [{}]}}"#, library.join(",")),
        )
        .unwrap();
        if let Some(old) = old {
            let games: Vec<bouncer::Game> = old
                .iter()
                .map(|n| bouncer::Game {
                    name: n.to_string(),
                    platform: "epic".to_string(),
                })
                .collect();
            cache::write_cache(&cache_dir.path().join("epic.json"), &games, "heroic_cache")
                .unwrap();
        }

        let reports = sync::sync_heroic(heroic_dir.path(), cache_dir.path());
        let epic = reports.iter().find(|r| r.platform == "epic").unwrap();
        if epic.added != *want_added || epic.removed != *want_removed {
            misses.push(format!(
                "{old:?} -> {new:?}: added {:?} removed {:?}",
                epic.added, epic.removed
            ));
        }
    }
    assert!(misses.is_empty(), "\n{}", misses.join("\n"));
}
