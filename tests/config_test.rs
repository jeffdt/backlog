use std::fs;
use tempfile::TempDir;

use bouncer::config;

#[test]
fn test_load_config_returns_none_when_missing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    assert!(config::load_config(&path).is_none());
}

#[test]
fn test_save_and_load_config() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    let cfg = config::Config {
        steam_api_key: "TESTKEY123".to_string(),
        steam_id: "76561197971395135".to_string(),
        itad_api_key: None,
    };
    config::save_config(&path, &cfg).unwrap();
    let loaded = config::load_config(&path).unwrap();
    assert_eq!(loaded.steam_api_key, "TESTKEY123");
    assert_eq!(loaded.steam_id, "76561197971395135");
}

#[test]
fn test_save_config_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("dir").join("config.json");
    let cfg = config::Config {
        steam_api_key: "KEY".to_string(),
        steam_id: "ID".to_string(),
        itad_api_key: None,
    };
    config::save_config(&path, &cfg).unwrap();
    assert!(path.exists());
}

#[test]
fn test_config_json_format_matches_python() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    let cfg = config::Config {
        steam_api_key: "ABC".to_string(),
        steam_id: "123".to_string(),
        itad_api_key: None,
    };
    config::save_config(&path, &cfg).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["steam_api_key"], "ABC");
    assert_eq!(parsed["steam_id"], "123");
    assert_eq!(parsed.as_object().unwrap().len(), 2);
}

#[test]
fn test_config_without_itad_key_still_loads() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path, r#"{"steam_api_key":"A","steam_id":"1"}"#).unwrap();
    let loaded = config::load_config(&path).unwrap();
    assert_eq!(loaded.itad_api_key, None);
}

#[test]
fn test_itad_key_round_trips() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    let cfg = config::Config {
        steam_api_key: "A".to_string(),
        steam_id: "1".to_string(),
        itad_api_key: Some("K".into()),
    };
    config::save_config(&path, &cfg).unwrap();
    let loaded = config::load_config(&path).unwrap();
    assert_eq!(loaded.itad_api_key.as_deref(), Some("K"));
}
