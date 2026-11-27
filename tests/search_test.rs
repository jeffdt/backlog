use bouncer::{LibraryEntry, search};

/// Titles taken from a real synced library, including the near-miss titles that
/// scattered fuzzy matching used to surface for unrelated queries.
const TITLES: &[&str] = &[
    "Kerbal Space Program",
    "Kizuna Encounter: Super Tag Battle",
    "Hollow Knight",
    "Hollow Knight: Silksong",
    "Mina the Hollower",
    "Dying Light The Following",
    "Hades",
    "Half-Life Deathmatch: Source",
    "Hard West 2",
    "Hard Reset",
    "Hellblade: Senua's Sacrifice",
    "DOOM",
    "DOOM II Enhanced",
    "Final Doom",
    "The Ultimate Doom",
    "Dragons of Flame",
    "Dungeons of Dredmor",
    "The Witcher: Enhanced Edition",
    "The Witcher 3: Wild Hunt — Remastered",
    "Thronebreaker: The Witcher Tales",
    "Nine Witches: Family Disruption",
    "Grand Theft Auto V",
    "Grand Theft Auto: Vice City",
    "Defense Grid: The Awakening",
    "Tomb Raider",
    "Rise of the Tomb Raider",
    "Shadow of the Tomb Raider: Definitive Edition",
    "Celeste",
    "Mystery Case Files: Moths to a Flame",
    "Portal",
    "Portal 2",
    "Company of Heroes: Tales of Valor",
    "Dead Cells",
    "Ni no Kuni Wrath of the White Witch™ Remastered",
    "TOEM",
    "Braid",
    "Batman™: Arkham Knight",
    "BioShock™ Remastered",
    "XCOM®: Chimera Squad",
    "Brütal Legend",
    "Schnellfahrstrecke Köln-Aachen",
    "Train Sim World® 5: Schnellfahrstrecke Köln-Aachen",
    "Train Sim World® 7: Schnellfahrstrecke Köln-Aachen Remastered",
];

fn library() -> Vec<LibraryEntry> {
    TITLES
        .iter()
        .map(|name| LibraryEntry {
            name: name.to_string(),
            platforms: vec!["steam".to_string()],
        })
        .collect()
}

fn result_names<'a>(query: &str, library: &'a [LibraryEntry]) -> Vec<&'a str> {
    search::fuzzy_search(query, library)
        .iter()
        .map(|r| r.game.name.as_str())
        .collect()
}

/// Fails once with every collected message, so a table run reports all misses at once.
fn assert_no_failures(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// (query, titles that must appear in the results)
const MUST_FIND: &[(&str, &[&str])] = &[
    ("celeste", &["Celeste"]),
    ("hades", &["Hades"]),
    ("portal", &["Portal", "Portal 2"]),
    ("kerb", &["Kerbal Space Program"]),
    ("kerbal", &["Kerbal Space Program"]),
    (
        "hollow",
        &[
            "Hollow Knight",
            "Hollow Knight: Silksong",
            "Mina the Hollower",
        ],
    ),
    ("ollow", &["Hollow Knight", "Mina the Hollower"]),
    (
        "witcher",
        &[
            "The Witcher: Enhanced Edition",
            "The Witcher 3: Wild Hunt — Remastered",
            "Thronebreaker: The Witcher Tales",
        ],
    ),
    (
        "doom",
        &[
            "DOOM",
            "DOOM II Enhanced",
            "Final Doom",
            "The Ultimate Doom",
        ],
    ),
    ("DoOm", &["DOOM", "Final Doom"]),
    ("CELESTE", &["Celeste"]),
    (
        "tomb raider",
        &[
            "Tomb Raider",
            "Rise of the Tomb Raider",
            "Shadow of the Tomb Raider: Definitive Edition",
        ],
    ),
    ("raider tomb", &["Tomb Raider"]),
    ("witcher 3", &["The Witcher 3: Wild Hunt — Remastered"]),
    ("hollow silk", &["Hollow Knight: Silksong"]),
    ("dead cells", &["Dead Cells"]),
    (
        "ni no kuni",
        &["Ni no Kuni Wrath of the White Witch™ Remastered"],
    ),
    ("hollw knight", &["Hollow Knight"]),
    ("ded cels", &["Dead Cells"]),
    (
        "gta",
        &["Grand Theft Auto V", "Grand Theft Auto: Vice City"],
    ),
    // Titles with a symbol or accent whose query ends the title.
    ("knight", &["Hollow Knight", "Batman™: Arkham Knight"]),
    ("arkham knight", &["Batman™: Arkham Knight"]),
    (
        "remastered",
        &[
            "BioShock™ Remastered",
            "The Witcher 3: Wild Hunt — Remastered",
            "Ni no Kuni Wrath of the White Witch™ Remastered",
            "Train Sim World® 7: Schnellfahrstrecke Köln-Aachen Remastered",
        ],
    ),
    ("squad", &["XCOM®: Chimera Squad"]),
    ("legend", &["Brütal Legend"]),
    ("brutal", &["Brütal Legend"]),
    ("brütal", &["Brütal Legend"]),
    (
        "aachen",
        &[
            "Schnellfahrstrecke Köln-Aachen",
            "Train Sim World® 5: Schnellfahrstrecke Köln-Aachen",
            "Train Sim World® 7: Schnellfahrstrecke Köln-Aachen Remastered",
        ],
    ),
    (
        "koln-aachen",
        &[
            "Schnellfahrstrecke Köln-Aachen",
            "Train Sim World® 5: Schnellfahrstrecke Köln-Aachen",
            "Train Sim World® 7: Schnellfahrstrecke Köln-Aachen Remastered",
        ],
    ),
];

/// (query, titles that must NOT appear in the results)
const MUST_NOT_FIND: &[(&str, &[&str])] = &[
    ("tomb raider", &["TOEM", "Braid"]),
    ("kerbal", &["Kizuna Encounter: Super Tag Battle"]),
    ("hollow", &["Dying Light The Following"]),
    (
        "hades",
        &[
            "Half-Life Deathmatch: Source",
            "Hard West 2",
            "Hard Reset",
            "Hellblade: Senua's Sacrifice",
        ],
    ),
    ("doom", &["Dragons of Flame", "Dungeons of Dredmor"]),
    ("celeste", &["Mystery Case Files: Moths to a Flame"]),
    ("portal", &["Company of Heroes: Tales of Valor"]),
    ("witcher", &["Nine Witches: Family Disruption"]),
    ("witcher 3", &["The Witcher: Enhanced Edition"]),
];

/// (query, title that must rank first)
const RANKS_FIRST: &[(&str, &str)] = &[
    ("tomb raider", "Tomb Raider"),
    ("kerbal", "Kerbal Space Program"),
    ("hades", "Hades"),
    ("celeste", "Celeste"),
    ("dead cells", "Dead Cells"),
    ("hollow silk", "Hollow Knight: Silksong"),
    ("witcher 3", "The Witcher 3: Wild Hunt — Remastered"),
    ("hollw knight", "Hollow Knight"),
];

#[test]
fn finds_expected_titles() {
    let library = library();
    let mut failures = Vec::new();
    for (query, expected) in MUST_FIND {
        let names = result_names(query, &library);
        for title in *expected {
            if !names.contains(title) {
                failures.push(format!("{query:?} should find {title:?}, got {names:?}"));
            }
        }
    }
    assert_no_failures(failures);
}

#[test]
fn excludes_unrelated_titles() {
    let library = library();
    let mut failures = Vec::new();
    for (query, unwanted) in MUST_NOT_FIND {
        let names = result_names(query, &library);
        for title in *unwanted {
            if names.contains(title) {
                failures.push(format!(
                    "{query:?} should not find {title:?}, got {names:?}"
                ));
            }
        }
    }
    assert_no_failures(failures);
}

#[test]
fn ranks_best_title_first() {
    let library = library();
    let mut failures = Vec::new();
    for (query, expected) in RANKS_FIRST {
        let names = result_names(query, &library);
        if names.first() != Some(expected) {
            failures.push(format!(
                "{query:?} should rank {expected:?} first, got {names:?}"
            ));
        }
    }
    assert_no_failures(failures);
}

#[test]
fn results_are_sorted_with_valid_highlights() {
    let library = library();
    let queries = MUST_FIND
        .iter()
        .chain(MUST_NOT_FIND)
        .map(|(q, _)| *q)
        .chain(RANKS_FIRST.iter().map(|(q, _)| *q));
    let mut failures = Vec::new();
    for query in queries {
        let results = search::fuzzy_search(query, &library);
        if results.windows(2).any(|w| w[0].score < w[1].score) {
            failures.push(format!("{query:?}: scores not descending"));
        }
        for r in &results {
            let len = r.game.name.chars().count() as u32;
            let in_range = r.match_indices.iter().all(|&i| i < len);
            if r.match_indices.is_empty() || !in_range {
                failures.push(format!(
                    "{query:?}: bad highlight {:?} for {:?}",
                    r.match_indices, r.game.name
                ));
            }
        }
    }
    assert_no_failures(failures);
}

#[test]
fn blank_query_returns_every_title() {
    let library = library();
    for query in ["", "   "] {
        let results = search::fuzzy_search(query, &library);
        assert_eq!(results.len(), library.len(), "query {query:?}");
        assert!(results.iter().all(|r| r.score == 0), "query {query:?}");
    }
}

#[test]
fn unmatched_query_returns_nothing() {
    assert!(search::fuzzy_search("zzzznotarealgame", &library()).is_empty());
}

#[test]
fn empty_library_returns_nothing() {
    assert!(search::fuzzy_search("test", &[]).is_empty());
}

#[test]
fn cross_store_dupe_yields_single_result() {
    let library = vec![LibraryEntry {
        name: "Card Shark".to_string(),
        platforms: vec!["epic".to_string(), "steam".to_string()],
    }];
    let results = search::fuzzy_search("card shark", &library);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].game.platforms, vec!["epic", "steam"]);
}
