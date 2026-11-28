//! Reads a Steam cart and judges each item against the library and all-time lows.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::itad::{BestPrice, LowPrice, Money};
use crate::{LibraryEntry, queue};

const GET_CART_URL: &str = "https://api.steampowered.com/IAccountCartService/GetCart/v1/";
const GET_ITEMS_URL: &str = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/";
const STEAM_USD: u64 = 1;

/// Whether a cart line is a package (one purchasable item) or a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Package,
    Bundle,
}

/// One line of the Steam cart, as the cart API reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct CartLine {
    pub kind: ItemKind,
    pub id: u64,
    pub cart_price: Money,
}

/// A package or bundle from the public store API.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreItem {
    pub name: String,
    /// The store's current price; for bundles this ignores games the user owns.
    pub live_price: Option<Money>,
    /// `(appid, name)` for each game the item contains.
    pub apps: Vec<(u64, String)>,
}

/// The call on whether a cart item is worth keeping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// At or under the all-time low from any store.
    AtLow,
    /// At or under Steam's own all-time low, but another store has gone lower.
    AtSteamLow,
    /// Steam itself has sold it cheaper.
    AboveSteamLow,
    /// Above the all-time low, with no Steam low known.
    AboveLow,
    Owned,
    NothingNew,
    Unknown,
}

/// What a game has cost and costs now, as IsThereAnyDeal reports it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppPrices {
    /// Cheapest current price across stores.
    pub best: Option<BestPrice>,
    pub steam_low: Option<Money>,
    pub low: Option<LowPrice>,
}

/// A game inside a cart item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CartApp {
    pub appid: u64,
    pub name: String,
    pub owned_on: Vec<String>,
    /// Dropped from a bundle by Steam because it is owned on Steam.
    pub excluded: bool,
    /// Cheapest current price across stores.
    pub best: Option<BestPrice>,
    pub steam_low: Option<Money>,
    pub low: Option<LowPrice>,
}

/// A judged cart item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CartRow {
    pub name: String,
    pub kind: ItemKind,
    pub price: Money,
    /// Sum of the cheapest current prices for the games this purchase would add.
    pub best_total: Option<Money>,
    /// Sum of Steam's all-time lows for the games this purchase would add.
    pub steam_low_total: Option<Money>,
    /// Sum of all-time lows for the games this purchase would add.
    pub low_total: Option<Money>,
    pub verdict: Verdict,
    pub apps: Vec<CartApp>,
}

/// One cart-wide total, with the number of items it leaves out for lack of a price.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Total {
    pub amount: Money,
    pub missing: usize,
}

/// What the whole cart costs at Steam, at today's best prices, at Steam's
/// all-time lows, and at all-time lows from any store.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CartTotals {
    pub cart: Money,
    pub best_now: Total,
    pub steam_low: Total,
    pub low: Total,
}

/// Sums each item's prices; assumes the cart is in a single currency.
pub fn totals(rows: &[CartRow]) -> CartTotals {
    let currency = rows
        .first()
        .map_or_else(|| "USD".to_string(), |r| r.price.currency.clone());
    let sum = |amount_of: fn(&CartRow) -> Option<&Money>| {
        let amounts: Vec<Option<f64>> = rows
            .iter()
            .map(|r| amount_of(r).map(|m| m.amount))
            .collect();
        Total {
            missing: amounts.iter().filter(|a| a.is_none()).count(),
            amount: Money {
                amount: amounts.into_iter().flatten().fold(0.0, |a, b| a + b),
                currency: currency.clone(),
            },
        }
    };
    CartTotals {
        cart: sum(|r| Some(&r.price)).amount,
        best_now: sum(|r| r.best_total.as_ref()),
        steam_low: sum(|r| r.steam_low_total.as_ref()),
        low: sum(|r| r.low_total.as_ref()),
    }
}

/// Sums one price per received game, or `None` if any is unknown or in another currency.
fn sum_prices<'a>(
    receiving: &[&'a CartApp],
    currency: &str,
    price_of: impl Fn(&'a CartApp) -> Option<&'a Money>,
) -> Option<Money> {
    let prices = receiving
        .iter()
        .map(|a| price_of(a))
        .collect::<Option<Vec<&Money>>>()?;
    prices
        .iter()
        .all(|m| m.currency == currency)
        .then(|| Money {
            amount: prices.iter().fold(0.0, |sum, m| sum + m.amount),
            currency: currency.to_string(),
        })
}

fn cents(v: &Value) -> Option<f64> {
    let text = v.as_str()?;
    Some(text.parse::<u64>().ok()? as f64 / 100.0)
}

/// Parses a `GetCart` response into its lines, skipping any that are neither package nor bundle.
pub fn parse_cart(v: &Value) -> Vec<CartLine> {
    let Some(items) = v
        .pointer("/response/cart/line_items")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let (kind, id) = if let Some(id) = item.get("packageid").and_then(Value::as_u64) {
                (ItemKind::Package, id)
            } else {
                (ItemKind::Bundle, item.get("bundleid")?.as_u64()?)
            };
            let price = item.get("price_when_added")?;
            let code = price.get("currency_code")?.as_u64()?;
            // shortcut: only USD is named; other currencies never compare equal to ITAD's US prices.
            let currency = if code == STEAM_USD {
                "USD".to_string()
            } else {
                format!("steam-currency-{code}")
            };
            Some(CartLine {
                kind,
                id,
                cart_price: Money {
                    amount: cents(price.get("amount_in_cents")?)?,
                    currency,
                },
            })
        })
        .collect()
}

/// Parses a `GetItems` response (fetched for the US store) keyed by kind and id.
pub fn parse_store_items(v: &Value) -> HashMap<(ItemKind, u64), StoreItem> {
    let Some(items) = v.pointer("/response/store_items").and_then(Value::as_array) else {
        return HashMap::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let kind = match item.get("item_type")?.as_u64()? {
                1 => ItemKind::Package,
                2 => ItemKind::Bundle,
                _ => return None,
            };
            if item.get("success")?.as_u64()? != 1 {
                return None;
            }
            let live_price = item
                .pointer("/best_purchase_option/final_price_in_cents")
                .and_then(cents)
                .map(|amount| Money {
                    amount,
                    currency: "USD".to_string(),
                });
            let apps = item
                .pointer("/included_items/included_apps")
                .and_then(Value::as_array)
                .map(|apps| {
                    apps.iter()
                        .filter_map(|a| {
                            Some((
                                a.get("appid")?.as_u64()?,
                                a.get("name")?.as_str()?.to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some((
                (kind, item.get("id")?.as_u64()?),
                StoreItem {
                    name: item.get("name")?.as_str()?.to_string(),
                    live_price,
                    apps,
                },
            ))
        })
        .collect()
}

/// Judges each cart line against the library and each game's prices keyed by app id.
///
/// Bundles are priced from the cart, because Steam leaves games the user owns
/// on Steam out of a bundle and only the cart price reflects that; those games
/// are marked excluded and left out of the comparison. Packages use the live
/// store price when there is one, so a sale that started after the game was
/// added still counts. A game owned on any platform marks the item `Owned`.
/// The price is compared to the all-time low from any store first, then to
/// Steam's own all-time low.
pub fn assess(
    lines: &[CartLine],
    items: &HashMap<(ItemKind, u64), StoreItem>,
    library: &[LibraryEntry],
    prices: &HashMap<u64, AppPrices>,
) -> Vec<CartRow> {
    let owned: HashMap<String, &Vec<String>> = library
        .iter()
        .map(|e| (queue::normalize_key(&e.name), &e.platforms))
        .collect();
    lines
        .iter()
        .map(|line| {
            let item = items.get(&(line.kind, line.id));
            let name = item.map_or_else(
                || format!("unknown {:?} {}", line.kind, line.id).to_lowercase(),
                |i| i.name.clone(),
            );
            let price = match (line.kind, item.and_then(|i| i.live_price.clone())) {
                (ItemKind::Package, Some(live)) => live,
                _ => line.cart_price.clone(),
            };
            let apps: Vec<CartApp> = item
                .map(|i| i.apps.as_slice())
                .unwrap_or_default()
                .iter()
                .map(|(appid, app_name)| {
                    let owned_on = owned
                        .get(&queue::normalize_key(app_name))
                        .map(|p| p.to_vec())
                        .unwrap_or_default();
                    let AppPrices {
                        best,
                        steam_low,
                        low,
                    } = prices.get(appid).cloned().unwrap_or_default();
                    CartApp {
                        appid: *appid,
                        name: app_name.clone(),
                        excluded: line.kind == ItemKind::Bundle
                            && owned_on.iter().any(|p| p == "steam"),
                        owned_on,
                        best,
                        steam_low,
                        low,
                    }
                })
                .collect();
            let receiving: Vec<&CartApp> = apps.iter().filter(|a| !a.excluded).collect();
            // A bundle with nothing new sums to zero; an item with no known games has no total.
            let total = |price_of: fn(&CartApp) -> Option<&Money>| {
                (!apps.is_empty())
                    .then(|| sum_prices(&receiving, &price.currency, price_of))
                    .flatten()
            };
            let best_total = total(|a| a.best.as_ref().map(|b| &b.price));
            let steam_low_total = total(|a| a.steam_low.as_ref());
            let low_total = total(|a| a.low.as_ref().map(|l| &l.price));
            // Half a cent of slack so float sums of cent amounts compare as equal.
            let at_or_under = |m: &Money| price.amount <= m.amount + 0.005;
            let verdict = if apps.is_empty() {
                Verdict::Unknown
            } else if receiving.is_empty() {
                Verdict::NothingNew
            } else if receiving.iter().any(|a| !a.owned_on.is_empty()) {
                Verdict::Owned
            } else {
                match (&low_total, &steam_low_total) {
                    (Some(low), _) if at_or_under(low) => Verdict::AtLow,
                    (_, Some(steam)) if at_or_under(steam) => Verdict::AtSteamLow,
                    (_, Some(_)) => Verdict::AboveSteamLow,
                    (Some(_), None) => Verdict::AboveLow,
                    (None, None) => Verdict::Unknown,
                }
            };
            CartRow {
                name,
                kind: line.kind,
                price,
                best_total,
                steam_low_total,
                low_total,
                verdict,
                apps,
            }
        })
        .collect()
}

fn get_json(url: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut response = ureq::get(url)
        .query_pairs(query.iter().copied())
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401 | 403) => {
                "Steam rejected the token; it may have expired, so fetch a fresh one".to_string()
            }
            e => format!("Steam request failed: {e}"),
        })?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Steam request failed: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("Steam request failed: bad response: {e}"))
}

/// Reads the `exp` claim (unix seconds) from a JWT such as Steam's `webapi_token`,
/// without verifying it. `None` when the token isn't a decodable JWT with `exp`.
pub fn token_expiry(token: &str) -> Option<i64> {
    let mut parts = token.split('.');
    let payload = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(p), Some(_), None) => p,
        _ => return None,
    };
    let json: Value = serde_json::from_slice(&decode_base64url(payload)?).ok()?;
    json.get("exp")?.as_i64()
}

/// Decodes base64url (padding optional), the encoding of JWT segments.
fn decode_base64url(text: &str) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut nbits = 0;
    let mut out = Vec::new();
    for c in text.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(v);
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    Some(out)
}

/// Fetches the cart of the user who owns `token` (a store `webapi_token`).
pub fn fetch_cart(token: &str) -> Result<Vec<CartLine>, String> {
    get_json(
        GET_CART_URL,
        &[("access_token", token), ("user_country", "US")],
    )
    .map(|v| parse_cart(&v))
    // The token rides in the URL, so keep it out of any error that echoes the request.
    .map_err(|e| e.replace(token, "<token>"))
}

/// Fetches public store details, including contained games, for every cart line.
pub fn fetch_store_items(
    lines: &[CartLine],
) -> Result<HashMap<(ItemKind, u64), StoreItem>, String> {
    let ids: Vec<Value> = lines
        .iter()
        .map(|l| match l.kind {
            ItemKind::Package => json!({ "packageid": l.id }),
            ItemKind::Bundle => json!({ "bundleid": l.id }),
        })
        .collect();
    let input = json!({
        "ids": ids,
        "context": { "language": "english", "country_code": "US" },
        "data_request": { "include_included_items": true },
    })
    .to_string();
    get_json(GET_ITEMS_URL, &[("input_json", &input)]).map(|v| parse_store_items(&v))
}
