use bouncer::itad::*;
use bouncer::output::{format_money, format_price_groups};
use serde_json::{Value, json};

const HADES: &str = "018d937f-6ee3-738a-b578-ddd7e9a0d24d";
fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

const BLASPHEMOUS: &str = "018d937f-6ee2-70f6-940c-6212ac74369e";

#[test]
fn classify_recognizes_each_input_kind() {
    let app = Input::SteamApp(1145350);
    let title = |t: &str| Input::Title(t.to_string());
    for (input, want) in [
        (
            "https://store.steampowered.com/app/1145350/Hades_II/",
            app.clone(),
        ),
        (
            "http://store.steampowered.com/app/1145350?l=en",
            app.clone(),
        ),
        ("1145350", app),
        (HADES, Input::ItadId(HADES.to_string())),
        (
            "Thank Goodness You're Here!",
            title("Thank Goodness You're Here!"),
        ),
        (
            "Shadow Gambit: The Cursed Crew",
            title("Shadow Gambit: The Cursed Crew"),
        ),
        ("STAR WARS™ Jedi", title("STAR WARS™ Jedi")),
        ("  Hades II ", title("Hades II")),
        ("99999999999999999999999", title("99999999999999999999999")),
    ] {
        assert_eq!(classify(input), want, "input: {input:?}");
    }
}

#[test]
fn parse_lookup_keeps_nulls() {
    let m = parse_lookup(&fixture(include_str!("fixtures/itad/lookup_title.json")));
    assert_eq!(m["Hades II"], Some(HADES.to_string()));
    assert_eq!(m["hades 2"], None);
    assert_eq!(m["zzqx"], None);
    let m = parse_lookup(&fixture(include_str!("fixtures/itad/lookup_steam.json")));
    assert_eq!(m["app/1145350"], Some(HADES.to_string()));
    assert_eq!(m["app/999999999"], None);
}

#[test]
fn parse_search_returns_candidates_in_order() {
    let refs = parse_search(&fixture(include_str!("fixtures/itad/search.json")));
    assert_eq!(refs.len(), 5);
    assert_eq!(refs[0].title, "Hades II");
    assert_eq!(refs[0].kind, "game");
    assert_eq!(refs[3].kind, "package");
}

#[test]
fn null_type_defaults_to_unknown() {
    let found = parse_search(&json!([
        {"id": "a", "title": "Baldur's Gate 3 - Digital Deluxe Edition", "type": null},
        {"id": "b", "title": "Hades II", "type": "game"}
    ]));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].kind, "unknown");
    let info = parse_info(&json!({"id": "a", "title": "X", "type": null})).unwrap();
    assert_eq!(info.kind, "unknown");
}

#[test]
fn parse_overview_reads_best_and_low() {
    let m = parse_overview(&fixture(include_str!("fixtures/itad/overview.json")));
    let (best, low) = &m[BLASPHEMOUS];
    let best = best.as_ref().unwrap();
    assert_eq!(best.price.amount, 6.14);
    assert_eq!(best.price.currency, "USD");
    assert_eq!(best.cut, 80);
    assert_eq!(best.shop, "GameBillet");
    let low = low.as_ref().unwrap();
    assert_eq!(low.price.amount, 1.99);
    assert_eq!(low.shop, "Gamer Thor");
    assert_eq!(low.date, "2025-01-04");
}

#[test]
fn parse_overview_handles_missing_current_and_lowest() {
    let m = parse_overview(&json!({"prices":[{"id":"x","current":null,"lowest":null}]}));
    assert_eq!(m["x"], (None, None));
}

#[test]
fn parse_steam_prices_reads_steam_deal() {
    let m = parse_steam_prices(&fixture(include_str!("fixtures/itad/prices_steam.json")));
    let s = &m[BLASPHEMOUS];
    assert_eq!(s.price.amount, 7.49);
    assert_eq!(s.regular.amount, 29.99);
    assert_eq!(s.cut, 75);
}

#[test]
fn parse_steam_prices_skips_games_without_steam_deal() {
    let m = parse_steam_prices(&json!([{"id":"x","deals":[]}]));
    assert!(!m.contains_key("x"));
}

#[test]
fn assemble_attaches_prices_per_input_and_keeps_duplicates() {
    let hades = GameRef {
        id: HADES.into(),
        title: "Hades II".into(),
        kind: "game".into(),
    };
    let overview = parse_overview(&fixture(include_str!("fixtures/itad/overview.json")));
    let steam = parse_steam_prices(&fixture(include_str!("fixtures/itad/prices_steam.json")));
    let groups = assemble(
        vec![
            ("Hades II".into(), MatchKind::Exact, vec![hades.clone()]),
            ("1145350".into(), MatchKind::Exact, vec![hades]),
            ("zzqx".into(), MatchKind::None, vec![]),
        ],
        &overview,
        &steam,
    );
    assert_eq!(groups.len(), 3);
    for g in &groups[..2] {
        assert_eq!(g.games.len(), 1);
        let game = &g.games[0];
        assert_eq!(game.id, HADES);
        assert_eq!(game.steam, steam.get(HADES).cloned());
        assert_eq!(game.steam.as_ref().unwrap().price.amount, 20.99);
        assert_eq!(game.best, overview[HADES].0);
        assert_eq!(game.low, overview[HADES].1);
    }
    assert_eq!(groups[0].input, "Hades II");
    assert_eq!(groups[1].input, "1145350");
    assert!(groups[2].games.is_empty());
    assert_eq!(groups[2].kind, MatchKind::None);
}

fn blasphemous_group() -> PriceGroup {
    let refs = vec![GameRef {
        id: BLASPHEMOUS.into(),
        title: "Blasphemous 2".into(),
        kind: "game".into(),
    }];
    assemble(
        vec![("Blasphemous 2".into(), MatchKind::Exact, refs)],
        &parse_overview(&fixture(include_str!("fixtures/itad/overview.json"))),
        &parse_steam_prices(&fixture(include_str!("fixtures/itad/prices_steam.json"))),
    )
    .remove(0)
}

#[test]
fn format_money_uses_symbol_for_usd_and_code_otherwise() {
    let m = |amount, currency: &str| Money {
        amount,
        currency: currency.into(),
    };
    assert_eq!(format_money(&m(20.99, "USD")), "$20.99");
    assert_eq!(format_money(&m(20.99, "EUR")), "20.99 EUR");
    assert_eq!(format_money(&m(5.0, "USD")), "$5.00");
}

#[test]
fn price_table_shows_steam_best_and_low() {
    let out = format_price_groups(&[blasphemous_group()]);
    assert!(out.lines().next().unwrap().starts_with("Title"), "{out}");
    for cell in [
        "$7.49 -75%",
        "$6.14 -80% GameBillet",
        "$1.99 Gamer Thor 2025-01-04",
    ] {
        assert!(out.contains(cell), "missing {cell} in {out}");
    }
}

#[test]
fn price_table_marks_missing_prices_and_lists_candidates() {
    let bare = |title: &str, kind: &str| GamePrice {
        id: "x".into(),
        title: title.into(),
        kind: kind.into(),
        steam: None,
        best: None,
        low: None,
    };
    let groups = [
        PriceGroup {
            input: "Free Thing".into(),
            kind: MatchKind::Exact,
            games: vec![bare("Free Thing", "game")],
        },
        PriceGroup {
            input: "hades 2".into(),
            kind: MatchKind::Candidates,
            games: vec![
                bare("Hades II", "game"),
                bare("Hades + Hades II", "package"),
            ],
        },
        PriceGroup {
            input: "zzqx".into(),
            kind: MatchKind::None,
            games: vec![],
        },
    ];
    let out = format_price_groups(&groups);
    let row = out.lines().find(|l| l.starts_with("Free Thing")).unwrap();
    assert_eq!(row.matches(" -").count(), 3, "{row}");
    assert!(
        out.lines()
            .any(|l| l == "hades 2: no exact match, candidates:"),
        "{out}"
    );
    let offset = |prefix: &str, kind: &str| {
        let l = out.lines().find(|l| l.starts_with(prefix)).unwrap();
        l.chars().count() - l.split_once(kind).unwrap().1.chars().count() - kind.len()
    };
    assert_eq!(
        offset("  Hades II", "game"),
        offset("  Hades + Hades II", "package"),
        "{out}"
    );
    assert!(!out.contains("zzqx"));
}

#[test]
fn zero_cut_steam_price_omits_percentage() {
    let mut group = blasphemous_group();
    let steam = group.games[0].steam.as_mut().unwrap();
    steam.price.amount = 29.99;
    steam.cut = 0;
    let out = format_price_groups(&[group]);
    let row = out.lines().nth(1).unwrap();
    assert!(row.contains("$29.99  "), "{row}");
    assert!(!row.contains("-0%"), "{row}");
}
