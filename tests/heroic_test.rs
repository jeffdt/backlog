use backlog::sources::heroic;
use tempfile::TempDir;
use std::fs;

fn write_json(dir: &TempDir, filename: &str, content: &str) -> std::path::PathBuf {
    let path = dir.path().join(filename);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn test_load_epic_extracts_titles() {
    let dir = TempDir::new().unwrap();
    let path = write_json(&dir, "legendary_library.json", r#"{
        "library": [
            {"title": "Celeste", "app_name": "abc123"},
            {"title": "Hades", "app_name": "def456"}
        ]
    }"#);
    let games = heroic::load_epic(&path);
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].name, "Celeste");
    assert_eq!(games[0].platform, "epic");
    assert_eq!(games[1].name, "Hades");
}

#[test]
fn test_load_gog_filters_redist() {
    let dir = TempDir::new().unwrap();
    let path = write_json(&dir, "gog_library.json", r#"{
        "games": [
            {"title": "Hollow Knight", "app_name": "hollow_knight"},
            {"title": "Galaxy Common Redistributables", "app_name": "gog-redist"}
        ]
    }"#);
    let games = heroic::load_gog(&path);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Hollow Knight");
    assert_eq!(games[0].platform, "gog");
}

#[test]
fn test_load_amazon_extracts_titles() {
    let dir = TempDir::new().unwrap();
    let path = write_json(&dir, "nile_library.json", r#"{
        "library": [
            {"title": "Castle on the Coast", "app_name": "amzn1.abc"}
        ]
    }"#);
    let games = heroic::load_amazon(&path);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Castle on the Coast");
    assert_eq!(games[0].platform, "amazon");
}

#[test]
fn test_load_returns_empty_when_file_missing() {
    let path = std::path::PathBuf::from("/nonexistent/path.json");
    assert!(heroic::load_epic(&path).is_empty());
    assert!(heroic::load_gog(&path).is_empty());
    assert!(heroic::load_amazon(&path).is_empty());
}

#[test]
fn test_load_epic_handles_empty_library() {
    let dir = TempDir::new().unwrap();
    let path = write_json(&dir, "legendary_library.json", r#"{"library": []}"#);
    assert!(heroic::load_epic(&path).is_empty());
}

#[test]
fn test_load_gog_handles_extra_fields_gracefully() {
    let dir = TempDir::new().unwrap();
    let path = write_json(&dir, "gog_library.json", r#"{
        "games": [
            {
                "title": "Fruitbus",
                "app_name": "fruitbus",
                "runner": "gog",
                "art_cover": "https://example.com/cover.jpg",
                "is_installed": true,
                "install": {"is_dlc": false}
            }
        ]
    }"#);
    let games = heroic::load_gog(&path);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Fruitbus");
}
