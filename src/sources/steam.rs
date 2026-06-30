use crate::Game;

pub const STEAM_API_URL: &str =
    "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/";

/// Fetches the owned game library for a Steam user via the Steam Web API.
pub fn fetch_steam_library(api_key: &str, steam_id: &str) -> Result<Vec<Game>, String> {
    let url = format!(
        "{}?key={}&steamid={}&include_appinfo=true&include_played_free_games=true&format=json",
        STEAM_API_URL, api_key, steam_id
    );
    let body: serde_json::Value = {
        let mut response =
            ureq::get(&url).call().map_err(|e| format!("HTTP request failed: {e}"))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("Failed to read response: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse response: {e}"))?
    };
    Ok(parse_steam_response(&body))
}

/// Parses a Steam `GetOwnedGames` JSON response into a list of `Game` entries.
pub fn parse_steam_response(data: &serde_json::Value) -> Vec<Game> {
    let Some(games) = data
        .get("response")
        .and_then(|r| r.get("games"))
        .and_then(|g| g.as_array())
    else {
        return Vec::new();
    };
    games
        .iter()
        .filter_map(|g| {
            let name = g.get("name")?.as_str()?;
            Some(Game {
                name: name.to_string(),
                platform: "steam".to_string(),
            })
        })
        .collect()
}
