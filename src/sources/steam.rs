use crate::Game;

pub const STEAM_API_URL: &str = "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/";

/// Fetches the owned game library for a Steam user via the Steam Web API.
pub fn fetch_steam_library(api_key: &str, steam_id: &str) -> Result<Vec<Game>, String> {
    let url = format!(
        "{}?key={}&steamid={}&include_appinfo=true&include_played_free_games=true&format=json",
        STEAM_API_URL, api_key, steam_id
    );
    let body: serde_json::Value = {
        let mut response = ureq::get(&url)
            .call()
            .map_err(|e| format!("HTTP request failed: {e}"))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("Failed to read response: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse response: {e}"))?
    };
    Ok(parse_steam_response(&body))
}

const RESOLVE_VANITY_URL: &str = "https://api.steampowered.com/ISteamUser/ResolveVanityURL/v1/";

/// Resolves a Steam vanity name to a 64-bit Steam ID via the Web API.
pub fn resolve_vanity_url(api_key: &str, vanity_name: &str) -> Result<String, String> {
    let url = format!(
        "{}?key={}&vanityurl={}",
        RESOLVE_VANITY_URL, api_key, vanity_name
    );
    let body: serde_json::Value = {
        let mut response = ureq::get(&url)
            .call()
            .map_err(|e| format!("HTTP request failed: {e}"))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("Failed to read response: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse response: {e}"))?
    };
    let resp = body.get("response").ok_or("missing response field")?;
    let success = resp.get("success").and_then(|s| s.as_u64()).unwrap_or(0);
    if success != 1 {
        return Err("vanity name not found".to_string());
    }
    resp.get("steamid")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or("missing steamid in response".to_string())
}

/// Parses a Steam profile URL or vanity name into a Steam ID.
///
/// Accepts these formats:
/// - `https://steamcommunity.com/profiles/76561198...` (numeric, returned as-is)
/// - `https://steamcommunity.com/id/vanityname` (resolved via API)
/// - `vanityname` (resolved via API)
/// - `76561198...` (numeric, returned as-is)
pub fn parse_steam_input(api_key: &str, input: &str) -> Result<String, String> {
    let input = input.trim().trim_end_matches('/');

    if let Some(rest) = input
        .strip_prefix("https://steamcommunity.com/profiles/")
        .or_else(|| input.strip_prefix("http://steamcommunity.com/profiles/"))
    {
        let id = rest.split('/').next().unwrap_or(rest);
        return Ok(id.to_string());
    }

    if let Some(rest) = input
        .strip_prefix("https://steamcommunity.com/id/")
        .or_else(|| input.strip_prefix("http://steamcommunity.com/id/"))
    {
        let vanity = rest.split('/').next().unwrap_or(rest);
        return resolve_vanity_url(api_key, vanity);
    }

    if input.chars().all(|c| c.is_ascii_digit()) && input.len() > 10 {
        return Ok(input.to_string());
    }

    resolve_vanity_url(api_key, input)
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
