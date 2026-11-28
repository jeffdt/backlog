//! IsThereAnyDeal input classification, response parsers, and result assembly.

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const API_BASE: &str = "https://api.isthereanydeal.com";
const USER_AGENT: &str = concat!("bouncer/", env!("CARGO_PKG_VERSION"));
const STEAM_SHOP_ID: u64 = 61;
const STEAM_APP_URL: &str = "store.steampowered.com/app/";

/// What a user-supplied query refers to.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    SteamApp(u64),
    ItadId(String),
    Title(String),
}

/// A price with its currency code.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Money {
    pub amount: f64,
    pub currency: String,
}

/// The current Steam deal for a game.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SteamPrice {
    pub price: Money,
    pub regular: Money,
    pub cut: u32,
    /// Steam's own all-time low, which ITAD reports without a date.
    pub store_low: Option<Money>,
}

/// The cheapest current price across shops.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BestPrice {
    pub price: Money,
    pub cut: u32,
    pub shop: String,
}

/// The all-time lowest price and when it happened.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LowPrice {
    pub price: Money,
    pub shop: String,
    /// `YYYY-MM-DD`.
    pub date: String,
}

/// A game identified by ITAD, before prices are attached.
#[derive(Debug, Clone, PartialEq)]
pub struct GameRef {
    pub id: String,
    pub title: String,
    pub kind: String,
}

/// A game with every price we know about.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GamePrice {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub steam: Option<SteamPrice>,
    pub best: Option<BestPrice>,
    pub low: Option<LowPrice>,
}

/// How well an input matched ITAD games.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    Exact,
    Candidates,
    None,
}

/// The result for one user input.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceGroup {
    pub input: String,
    #[serde(rename = "match")]
    pub kind: MatchKind,
    pub games: Vec<GamePrice>,
}

fn is_itad_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Classifies a raw query as a Steam app, an ITAD id, or a title.
pub fn classify(input: &str) -> Input {
    let s = input.trim();
    if let Some((_, rest)) = s.split_once(STEAM_APP_URL) {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(id) = digits.parse() {
            return Input::SteamApp(id);
        }
    }
    // shortcut: a game titled only with digits (e.g. "140") is read as an app ID;
    // pass its store URL or ITAD ID instead.
    if s.bytes().all(|b| b.is_ascii_digit())
        && let Ok(id) = s.parse()
    {
        return Input::SteamApp(id);
    }
    if is_itad_id(s) {
        return Input::ItadId(s.to_string());
    }
    Input::Title(s.to_string())
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

fn money(v: &Value) -> Option<Money> {
    Some(Money {
        amount: v.get("amount")?.as_f64()?,
        currency: str_of(v, "currency")?,
    })
}

fn shop_name(v: &Value) -> Option<String> {
    str_of(v.get("shop")?, "name")
}

fn cut(v: &Value) -> Option<u32> {
    u32::try_from(v.get("cut")?.as_u64()?).ok()
}

/// Parses a game info response (or one search result); a missing type reads as "unknown".
pub fn parse_info(v: &Value) -> Option<GameRef> {
    Some(GameRef {
        id: str_of(v, "id")?,
        title: str_of(v, "title")?,
        kind: str_of(v, "type").unwrap_or_else(|| "unknown".to_string()),
    })
}

/// Parses a lookup response, keeping unmatched keys as `None`.
pub fn parse_lookup(v: &Value) -> HashMap<String, Option<String>> {
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, id)| (k.clone(), id.as_str().map(str::to_string)))
                .collect()
        })
        .unwrap_or_default()
}

/// Parses a search response into candidates, in ITAD's order.
pub fn parse_search(v: &Value) -> Vec<GameRef> {
    v.as_array()
        .map(|a| a.iter().filter_map(parse_info).collect())
        .unwrap_or_default()
}

fn best_price(v: &Value) -> Option<BestPrice> {
    Some(BestPrice {
        price: money(v.get("price")?)?,
        cut: cut(v)?,
        shop: shop_name(v)?,
    })
}

fn low_price(v: &Value) -> Option<LowPrice> {
    Some(LowPrice {
        price: money(v.get("price")?)?,
        shop: shop_name(v)?,
        date: v.get("timestamp")?.as_str()?.get(..10)?.to_string(),
    })
}

/// Parses a price overview into best current and all-time low prices, keyed by game id.
pub fn parse_overview(v: &Value) -> HashMap<String, (Option<BestPrice>, Option<LowPrice>)> {
    v.get("prices")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let id = str_of(p, "id")?;
                    let best = p.get("current").and_then(best_price);
                    let low = p.get("lowest").and_then(low_price);
                    Some((id, (best, low)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parses a prices response into each game's first Steam deal; games without one are absent.
pub fn parse_steam_prices(v: &Value) -> HashMap<String, SteamPrice> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| {
                    let id = str_of(g, "id")?;
                    let deal = g.get("deals")?.as_array()?.iter().find(|d| {
                        d.get("shop")
                            .and_then(|s| s.get("id"))
                            .and_then(Value::as_u64)
                            == Some(STEAM_SHOP_ID)
                    })?;
                    Some((
                        id,
                        SteamPrice {
                            price: money(deal.get("price")?)?,
                            regular: money(deal.get("regular")?)?,
                            cut: cut(deal)?,
                            store_low: deal.get("storeLow").and_then(money),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Attaches prices to each resolved input, preserving input order and duplicates.
pub fn assemble(
    resolved: Vec<(String, MatchKind, Vec<GameRef>)>,
    overview: &HashMap<String, (Option<BestPrice>, Option<LowPrice>)>,
    steam: &HashMap<String, SteamPrice>,
) -> Vec<PriceGroup> {
    resolved
        .into_iter()
        .map(|(input, kind, refs)| PriceGroup {
            input,
            kind,
            games: refs
                .into_iter()
                .map(|r| {
                    let (best, low) = overview.get(&r.id).cloned().unwrap_or((None, None));
                    GamePrice {
                        steam: steam.get(&r.id).cloned(),
                        id: r.id,
                        title: r.title,
                        kind: r.kind,
                        best,
                        low,
                    }
                })
                .collect(),
        })
        .collect()
}

/// Sends one ITAD request (a POST when `body` is given), returning `None` on 404.
fn call(
    api_key: &str,
    path: &str,
    query: &[(&str, &str)],
    body: Option<&[String]>,
) -> Result<Option<Value>, String> {
    let fail = |e: &dyn std::fmt::Display| format!("ITAD request failed: {e}");
    let url = format!("{API_BASE}{path}");
    let result = match body {
        None => ureq::get(url)
            .header("User-Agent", USER_AGENT)
            .query("key", api_key)
            .query_pairs(query.iter().copied())
            .call(),
        Some(body) => ureq::post(url)
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .query("key", api_key)
            .query_pairs(query.iter().copied())
            .send(serde_json::to_string(body).map_err(|e| fail(&e))?),
    };
    let mut response = match result {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(fail(&e)),
    };
    let text = response.body_mut().read_to_string().map_err(|e| fail(&e))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| fail(&format!("bad response: {e}")))
}

/// Like [`call`], but a 404 is an error.
fn call_found(
    api_key: &str,
    path: &str,
    query: &[(&str, &str)],
    body: Option<&[String]>,
) -> Result<Value, String> {
    call(api_key, path, query, body)?
        .ok_or_else(|| format!("ITAD request failed: {}", ureq::Error::StatusCode(404)))
}

/// POSTs a batch and parses the response, skipping the request when `body` is empty.
fn post_batch<T: Default>(
    api_key: &str,
    path: &str,
    query: &[(&str, &str)],
    body: &[String],
    parse: impl Fn(&Value) -> T,
) -> Result<T, String> {
    if body.is_empty() {
        return Ok(T::default());
    }
    Ok(parse(&call_found(api_key, path, query, Some(body))?))
}

fn distinct(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

/// Resolves every input against ITAD and attaches Steam, best, and all-time-low
/// prices, one group per input in input order.
pub fn check_prices(api_key: &str, inputs: &[String]) -> Result<Vec<PriceGroup>, String> {
    let classified: Vec<Input> = inputs.iter().map(|raw| classify(raw)).collect();

    let titles = distinct(classified.iter().filter_map(|i| match i {
        Input::Title(t) => Some(t.clone()),
        _ => None,
    }));
    let apps = distinct(classified.iter().filter_map(|i| match i {
        Input::SteamApp(id) => Some(format!("app/{id}")),
        _ => None,
    }));
    let title_ids = post_batch(api_key, "/lookup/id/title/v1", &[], &titles, parse_lookup)?;
    let shop_path = format!("/lookup/id/shop/{STEAM_SHOP_ID}/v1");
    let app_ids = post_batch(api_key, &shop_path, &[], &apps, parse_lookup)?;

    let hits: Vec<Option<String>> = classified
        .iter()
        .map(|i| match i {
            Input::Title(t) => title_ids.get(t).cloned().flatten(),
            Input::SteamApp(id) => app_ids.get(&format!("app/{id}")).cloned().flatten(),
            Input::ItadId(id) => Some(id.clone()),
        })
        .collect();

    // A 404 from info makes that input unmatched; other failures abort the run.
    let mut infos = HashMap::new();
    for id in distinct(hits.iter().flatten().cloned()) {
        let info = call(api_key, "/games/info/v2", &[("id", &id)], None)?;
        infos.insert(id, info.as_ref().and_then(parse_info));
    }
    let misses = classified.iter().zip(&hits).filter_map(|pair| match pair {
        (Input::Title(t), None) => Some(t.clone()),
        _ => None,
    });
    let mut searches = HashMap::new();
    for title in distinct(misses) {
        let query = [("title", title.as_str()), ("results", "5")];
        let found = parse_search(&call_found(api_key, "/games/search/v1", &query, None)?);
        searches.insert(title, found);
    }

    let resolved: Vec<(String, MatchKind, Vec<GameRef>)> = inputs
        .iter()
        .zip(classified)
        .zip(hits)
        .map(|((input, class), hit)| {
            let (kind, refs) = match (hit, class) {
                (Some(id), _) => match infos[&id].clone() {
                    Some(r) => (MatchKind::Exact, vec![r]),
                    None => (MatchKind::None, vec![]),
                },
                (None, Input::Title(t)) => match searches[&t].clone() {
                    found if found.is_empty() => (MatchKind::None, found),
                    found => (MatchKind::Candidates, found),
                },
                (None, _) => (MatchKind::None, vec![]),
            };
            (input.clone(), kind, refs)
        })
        .collect();

    let ids = distinct(
        resolved
            .iter()
            .flat_map(|(_, _, refs)| refs.iter().map(|r| r.id.clone())),
    );
    let shops = STEAM_SHOP_ID.to_string();
    let us = [("country", "US")];
    let overview = post_batch(api_key, "/games/overview/v2", &us, &ids, parse_overview)?;
    let prices_query = [("country", "US"), ("shops", shops.as_str())];
    let steam = post_batch(
        api_key,
        "/games/prices/v3",
        &prices_query,
        &ids,
        parse_steam_prices,
    )?;
    Ok(assemble(resolved, &overview, &steam))
}
