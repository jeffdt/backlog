use bouncer::Game;
use bouncer::cart::*;
use bouncer::itad::{BestPrice, LowPrice, Money};
use bouncer::loader::dedupe;
use bouncer::output::format_cart_rows;
use serde_json::Value;
use std::collections::HashMap;

const ROGUE_PRINCE: u64 = 2717880;
const LOST_CROWN: u64 = 2751000;
const BLASPHEMOUS: u64 = 774361;
const BLASPHEMOUS_2: u64 = 2114740;
const DETROIT: u64 = 1222140;

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn lines() -> Vec<CartLine> {
    parse_cart(&fixture(include_str!("fixtures/steam_cart/get_cart.json")))
}

fn items() -> HashMap<(ItemKind, u64), StoreItem> {
    parse_store_items(&fixture(include_str!("fixtures/steam_cart/get_items.json")))
}

fn usd(amount: f64) -> Money {
    Money {
        amount,
        currency: "USD".to_string(),
    }
}

fn library(owned: &[(&str, &str)]) -> Vec<bouncer::LibraryEntry> {
    dedupe(
        owned
            .iter()
            .map(|(name, platform)| Game {
                name: name.to_string(),
                platform: platform.to_string(),
                ..Default::default()
            })
            .collect(),
    )
}

fn low(amount: f64) -> LowPrice {
    LowPrice {
        price: usd(amount),
        shop: "Steam".to_string(),
        date: "2025-11-27".to_string(),
    }
}

/// All-time lows from any store only, with no current best price or Steam low.
fn lows(known: &[(u64, f64)]) -> HashMap<u64, AppPrices> {
    known
        .iter()
        .map(|(id, amount)| {
            let prices = AppPrices {
                low: Some(low(*amount)),
                ..Default::default()
            };
            (*id, prices)
        })
        .collect()
}

fn with_steam_lows(
    mut prices: HashMap<u64, AppPrices>,
    steam: &[(u64, f64)],
) -> HashMap<u64, AppPrices> {
    for (id, amount) in steam {
        prices.entry(*id).or_default().steam_low = Some(usd(*amount));
    }
    prices
}

#[test]
fn parse_cart_reads_packages_bundles_and_prices() {
    let got: Vec<(ItemKind, u64, f64)> = lines()
        .iter()
        .map(|l| (l.kind, l.id, l.cart_price.amount))
        .collect();
    assert_eq!(
        got,
        vec![
            (ItemKind::Package, 423337, 3.99),
            (ItemKind::Bundle, 43205, 8.09),
            (ItemKind::Bundle, 33529, 6.74),
        ]
    );
    assert!(lines().iter().all(|l| l.cart_price.currency == "USD"));
}

#[test]
fn parse_store_items_reads_contained_games_and_live_price() {
    let items = items();
    let pop = &items[&(ItemKind::Bundle, 43205)];
    assert_eq!(pop.name, "Prince of Persia New Era");
    assert_eq!(
        pop.apps,
        vec![
            (ROGUE_PRINCE, "The Rogue Prince of Persia".to_string()),
            (LOST_CROWN, "Prince of Persia The Lost Crown".to_string()),
        ]
    );
    // The public bundle price ignores what the user owns, unlike the cart price.
    assert_eq!(pop.live_price, Some(usd(35.08)));
    assert_eq!(
        items[&(ItemKind::Package, 423337)].live_price,
        Some(usd(3.99))
    );
}

/// Each row: library, all-time lows, then the verdicts for Detroit (package),
/// Prince of Persia New Era (bundle, cart $8.09), and the Blasphemous bundle
/// (cart $6.74), in cart order.
type Case = (
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [(u64, f64)],
    [Verdict; 3],
);

const CASES: &[Case] = &[
    (
        "games owned on steam drop out of bundles",
        &[
            ("Prince of Persia The Lost Crown", "steam"),
            ("Blasphemous", "steam"),
        ],
        &[(DETROIT, 3.99), (ROGUE_PRINCE, 8.09), (BLASPHEMOUS_2, 5.00)],
        [Verdict::AtLow, Verdict::AtLow, Verdict::AboveLow],
    ),
    (
        "owning nothing compares the whole bundle to summed lows",
        &[],
        &[
            (DETROIT, 2.00),
            (ROGUE_PRINCE, 5.00),
            (LOST_CROWN, 10.00),
            (BLASPHEMOUS, 2.00),
            (BLASPHEMOUS_2, 3.00),
        ],
        [Verdict::AboveLow, Verdict::AtLow, Verdict::AboveLow],
    ),
    (
        "owned on another store still counts as owned",
        &[
            ("Detroit: Become Human", "gog"),
            ("The Rogue Prince of Persia", "epic"),
            ("Prince of Persia The Lost Crown", "steam"),
            ("Blasphemous", "steam"),
            ("Blasphemous 2", "steam"),
        ],
        &[(DETROIT, 1.00), (ROGUE_PRINCE, 1.00)],
        [Verdict::Owned, Verdict::Owned, Verdict::NothingNew],
    ),
    (
        "owned on steam and another store still drops out of a bundle",
        &[
            ("Prince of Persia The Lost Crown", "epic"),
            ("Prince of Persia The Lost Crown", "steam"),
        ],
        &[(DETROIT, 3.99), (ROGUE_PRINCE, 9.00)],
        [Verdict::AtLow, Verdict::AtLow, Verdict::Unknown],
    ),
    (
        "library casing does not matter",
        &[("prince of persia the lost crown", "steam")],
        &[(ROGUE_PRINCE, 9.00)],
        [Verdict::Unknown, Verdict::AtLow, Verdict::Unknown],
    ),
    (
        "a missing low for any received game is unknown",
        &[],
        &[(ROGUE_PRINCE, 9.00), (BLASPHEMOUS, 9.00)],
        [Verdict::Unknown, Verdict::Unknown, Verdict::Unknown],
    ),
];

#[test]
fn assess_judges_each_cart_item() {
    let (lines, items) = (lines(), items());
    let mut misses = Vec::new();
    for (name, owned, known_lows, want) in CASES {
        let rows = assess(&lines, &items, &library(owned), &lows(known_lows));
        let got: Vec<Verdict> = rows.iter().map(|r| r.verdict).collect();
        if got != want {
            misses.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    }
    assert!(misses.is_empty(), "\n{}", misses.join("\n"));
}

/// Each row: all-time lows from any store, Steam's own lows, then the
/// verdicts in cart order, with Lost Crown and Blasphemous owned on Steam so
/// each bundle receives one game (Rogue Prince at $8.09, Blasphemous 2 at $6.74).
type SteamLowCase = (
    &'static str,
    &'static [(u64, f64)],
    &'static [(u64, f64)],
    [Verdict; 3],
);

const STEAM_LOW_CASES: &[SteamLowCase] = &[
    (
        "beating Steam's low but not a reseller's is at Steam low",
        &[(DETROIT, 3.64), (ROGUE_PRINCE, 8.99), (BLASPHEMOUS_2, 1.99)],
        &[(DETROIT, 3.99), (ROGUE_PRINCE, 8.99), (BLASPHEMOUS_2, 7.49)],
        [Verdict::AtSteamLow, Verdict::AtLow, Verdict::AtSteamLow],
    ),
    (
        "Steam having sold it cheaper is above Steam low",
        &[(DETROIT, 2.00), (ROGUE_PRINCE, 5.00), (BLASPHEMOUS_2, 1.99)],
        &[(DETROIT, 2.50), (ROGUE_PRINCE, 6.00), (BLASPHEMOUS_2, 5.00)],
        [
            Verdict::AboveSteamLow,
            Verdict::AboveSteamLow,
            Verdict::AboveSteamLow,
        ],
    ),
    (
        "a Steam low alone still judges the price",
        &[],
        &[(DETROIT, 3.99), (ROGUE_PRINCE, 7.00)],
        [
            Verdict::AtSteamLow,
            Verdict::AboveSteamLow,
            Verdict::Unknown,
        ],
    ),
];

#[test]
fn assess_compares_to_steam_low_after_the_all_time_low() {
    let (lines, items) = (lines(), items());
    let owned = library(&[
        ("Prince of Persia The Lost Crown", "steam"),
        ("Blasphemous", "steam"),
    ]);
    let mut misses = Vec::new();
    for (name, any_lows, steam_lows, want) in STEAM_LOW_CASES {
        let prices = with_steam_lows(lows(any_lows), steam_lows);
        let rows = assess(&lines, &items, &owned, &prices);
        let got: Vec<Verdict> = rows.iter().map(|r| r.verdict).collect();
        if got != want {
            misses.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    }
    assert!(misses.is_empty(), "\n{}", misses.join("\n"));
}

#[test]
fn package_uses_live_price_and_bundle_uses_cart_price() {
    let mut lines = lines();
    lines[0].cart_price = usd(39.99);
    let rows = assess(&lines, &items(), &[], &HashMap::new());
    assert_eq!(rows[0].price, usd(3.99));
    assert_eq!(rows[1].price, usd(8.09));
}

#[test]
fn item_missing_from_store_is_unknown() {
    let rows = assess(&lines(), &HashMap::new(), &[], &HashMap::new());
    assert!(rows.iter().all(|r| r.verdict == Verdict::Unknown));
    assert_eq!(rows[0].name, "unknown package 423337");
}

#[test]
fn item_without_a_game_list_is_unknown_not_nothing_new() {
    let mut items = items();
    items
        .get_mut(&(ItemKind::Bundle, 43205))
        .unwrap()
        .apps
        .clear();
    let rows = assess(&lines(), &items, &[], &lows(&[(DETROIT, 1.0)]));
    assert_eq!(rows[1].verdict, Verdict::Unknown);
    assert_eq!((&rows[1].best_total, &rows[1].low_total), (&None, &None));
}

#[test]
fn totals_sum_cart_best_now_and_both_lows_counting_unpriced_items() {
    let best = |amount: f64| BestPrice {
        price: usd(amount),
        cut: 50,
        shop: "GOG".to_string(),
    };
    let prices = HashMap::from([
        (
            DETROIT,
            AppPrices {
                best: Some(best(3.00)),
                steam_low: Some(usd(3.50)),
                low: Some(low(2.50)),
            },
        ),
        (
            ROGUE_PRINCE,
            AppPrices {
                best: Some(best(9.50)),
                ..Default::default()
            },
        ),
    ]);
    // Owning both Blasphemous games on Steam leaves that bundle nothing new, which costs nothing.
    let owned = library(&[
        ("Prince of Persia The Lost Crown", "steam"),
        ("Blasphemous", "steam"),
        ("Blasphemous 2", "steam"),
    ]);
    let t = totals(&assess(&lines(), &items(), &owned, &prices));
    assert_eq!(
        t,
        CartTotals {
            cart: usd(3.99 + 8.09 + 6.74),
            best_now: Total {
                amount: usd(3.00 + 9.50),
                missing: 0,
            },
            steam_low: Total {
                amount: usd(3.50),
                missing: 1,
            },
            low: Total {
                amount: usd(2.50),
                missing: 1,
            },
        }
    );
}

#[test]
fn non_usd_cart_never_compares_to_us_lows() {
    let mut lines = lines();
    lines[1].cart_price.currency = "steam-currency-3".to_string();
    let rows = assess(
        &lines,
        &items(),
        &[],
        &lows(&[(ROGUE_PRINCE, 50.0), (LOST_CROWN, 50.0)]),
    );
    assert_eq!(rows[1].verdict, Verdict::Unknown);
}

#[test]
fn format_cart_rows_lists_bundle_games_under_the_bundle() {
    let rows = assess(
        &lines(),
        &items(),
        &library(&[
            ("Prince of Persia The Lost Crown", "steam"),
            ("Detroit: Become Human", "gog"),
        ]),
        &with_steam_lows(
            lows(&[
                (DETROIT, 3.99),
                (ROGUE_PRINCE, 8.09),
                (BLASPHEMOUS, 1.0),
                (BLASPHEMOUS_2, 5.0),
            ]),
            &[
                (ROGUE_PRINCE, 8.99),
                (BLASPHEMOUS, 1.99),
                (BLASPHEMOUS_2, 7.49),
            ],
        ),
    );
    let want = "\
Item                                         Cart   Steam low  All-time low
Detroit: Become Human                        $3.99  -          $3.99 Steam 2025-11-27  already owned (gog)
Prince of Persia New Era (bundle)            $8.09  $8.99      $8.09 Steam 2025-11-27  at low
  The Rogue Prince of Persia                        $8.99      $8.09 Steam 2025-11-27
  Prince of Persia The Lost Crown                                                      owned on steam, excluded
Blasphemous + Blasphemous 2 Bundle (bundle)  $6.74  $9.48      $6.00                   at Steam low
  Blasphemous                                       $1.99      $1.00 Steam 2025-11-27
  Blasphemous 2                                     $7.49      $5.00 Steam 2025-11-27

Cart total          $18.82
At best prices now   $0.00  (3 items with no price left out)
At Steam lows       $18.47  (1 item with no price left out)
At all-time lows    $18.08";
    assert_eq!(format_cart_rows(&rows), want);
}

#[test]
fn token_expiry_reads_jwt_exp() {
    const CASES: &[(&str, Option<i64>)] = &[
        (
            "eyAidHlwIjogIkpXVCIgfQ.eyJleHAiOjE3MDAwMDAwMDB9.sig",
            Some(1700000000),
        ),
        ("h.eyJleHAiOjE3MDAwMDAwMDAxfQ==.s", Some(17000000001)),
        ("h.eyJleHAiOjE3MDAwMDAwMDAxfQ.s", Some(17000000001)),
        (
            "h.eyJpc3MiOiJzdGVhbSIsInN1YiI6Ijc2NTYxMTkiLCJhdWQiOlsid2ViOnN0b3JlIl0sImV4cCI6MTc2NDQwMDAwMCwieCI6Ij8_Pj5-fiJ9.s",
            Some(1764400000),
        ),
        ("h.eyJpc3MiOiJzdGVhbSJ9.s", None),
        ("eyJleHAiOjE3MDAwMDAwMDB9", None),
        ("h.not*base64.s", None),
        ("h.bm90IGpzb24.s", None),
        ("", None),
    ];
    let misses: Vec<String> = CASES
        .iter()
        .filter_map(|&(token, want)| {
            let got = token_expiry(token);
            (got != want).then(|| format!("{token:?}: want {want:?}, got {got:?}"))
        })
        .collect();
    assert!(misses.is_empty(), "\n{}", misses.join("\n"));
}
