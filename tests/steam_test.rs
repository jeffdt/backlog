use backlog::sources::steam;

#[test]
fn test_parse_steam_response_extracts_games() {
    let data: serde_json::Value = serde_json::from_str(
        r#"{
        "response": {
            "game_count": 3,
            "games": [
                {"appid": 70, "name": "Half-Life"},
                {"appid": 220, "name": "Half-Life 2"},
                {"appid": 240, "name": "Counter-Strike: Source"}
            ]
        }
    }"#,
    )
    .unwrap();
    let games = steam::parse_steam_response(&data);
    assert_eq!(games.len(), 3);
    assert_eq!(games[0].name, "Half-Life");
    assert_eq!(games[0].platform, "steam");
    assert_eq!(games[2].name, "Counter-Strike: Source");
}

#[test]
fn test_parse_steam_response_handles_empty() {
    let data: serde_json::Value =
        serde_json::from_str(r#"{"response": {"game_count": 0, "games": []}}"#).unwrap();
    let games = steam::parse_steam_response(&data);
    assert!(games.is_empty());
}

#[test]
fn test_parse_steam_response_handles_missing_response() {
    let data: serde_json::Value = serde_json::from_str(r#"{}"#).unwrap();
    let games = steam::parse_steam_response(&data);
    assert!(games.is_empty());
}

#[test]
fn test_parse_steam_response_handles_missing_games() {
    let data: serde_json::Value = serde_json::from_str(r#"{"response": {}}"#).unwrap();
    let games = steam::parse_steam_response(&data);
    assert!(games.is_empty());
}
