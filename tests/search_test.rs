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
    "Stardew Valley",
    "Baldur's Gate: Enhanced Edition",
    "Baldur's Gate 3",
    "Cyberpunk 2077",
    "Keylocker | Turn Based Cyberpunk Action",
    "Star Wars: Knights of the Old Republic",
    "Guacamelee! Super Turbo Championship Edition",
    "Space Warlord Organ Trading Simulator",
    "My Time at Portia",
    "Ōkami HD",
    "ŚWIAT Racer",
    "Żółw Ninja",
    "Łódź Story",
    "Ørsted Islands",
    "Đorđe Adventure",
    "Æon Flux",
    "Straße Racer",
    "Œuvre Collection",
    "Grand Theft Auto V Enhanced",
    "Grand Theft Auto IV: The Complete Edition",
    "Gunslugs 3: Rogue Tactics",
    "Gas Station Simulator",
    "Gravitar: Recharged",
    "Death's Gambit - Afterlife",
    "Half-Life 2",
    "Trine 2",
    "Assassin’s Creed",
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
    // Typos: substitutions, transpositions and dropped letters.
    ("witcehr", &["The Witcher 3: Wild Hunt — Remastered"]),
    ("hadse", &["Hades"]),
    (
        "baldrus gate",
        &["Baldur's Gate 3", "Baldur's Gate: Enhanced Edition"],
    ),
    // Accented and uppercase queries fold the same way titles do.
    ("OKAMI", &["Ōkami HD"]),
    ("ōkami", &["Ōkami HD"]),
    ("łódź", &["Łódź Story"]),
    ("æon", &["Æon Flux"]),
    ("straße", &["Straße Racer"]),
    // Word-initial acronyms.
    (
        "gta",
        &[
            "Grand Theft Auto V",
            "Grand Theft Auto V Enhanced",
            "Grand Theft Auto IV: The Complete Edition",
            "Grand Theft Auto: Vice City",
            "Defense Grid: The Awakening",
        ],
    ),
    (
        "tr",
        &[
            "Tomb Raider",
            "Rise of the Tomb Raider",
            "Shadow of the Tomb Raider: Definitive Edition",
            "Trine 2",
            "Straße Racer",
        ],
    ),
    ("ttr", &["Rise of the Tomb Raider"]),
    ("GTA", &["Grand Theft Auto V"]),
    ("ksp", &["Kerbal Space Program"]),
    ("p2", &["Portal 2"]),
    ("hl", &["Half-Life 2", "Half-Life Deathmatch: Source"]),
    ("hk", &["Hollow Knight", "Hollow Knight: Silksong"]),
    ("ak", &["Batman™: Arkham Knight"]),
    ("xcs", &["XCOM®: Chimera Squad"]),
    ("oh", &["Ōkami HD"]),
    ("dga", &["Death's Gambit - Afterlife"]),
    ("ac", &["Assassin’s Creed"]),
    ("bg3", &["Baldur's Gate 3"]),
    ("swkotor", &["Star Wars: Knights of the Old Republic"]),
    ("gta vice", &["Grand Theft Auto: Vice City"]),
    // Two chars with no literal or acronym hit still fall back to fuzzy.
    ("kp", &["Kerbal Space Program"]),
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
    // Typo tolerance must not drag in loosely related titles.
    (
        "kerbel",
        &[
            "Kizuna Encounter: Super Tag Battle",
            "Star Wars: Knights of the Old Republic",
        ],
    ),
    ("stardwe", &["Star Wars: Knights of the Old Republic"]),
    ("celeset", &["Mystery Case Files: Moths to a Flame"]),
    (
        "portla",
        &[
            "Space Warlord Organ Trading Simulator",
            "Company of Heroes: Tales of Valor",
        ],
    ),
    (
        "hadse",
        &["The Witcher: Enhanced Edition", "Baldur's Gate 3"],
    ),
    (
        "ded cels",
        &["Train Sim World® 7: Schnellfahrstrecke Köln-Aachen Remastered"],
    ),
    (
        "gta",
        &[
            "Guacamelee! Super Turbo Championship Edition",
            "Baldur's Gate: Enhanced Edition",
            "Gunslugs 3: Rogue Tactics",
            "Gas Station Simulator",
            "Gravitar: Recharged",
            "Death's Gambit - Afterlife",
        ],
    ),
    ("ksp", &["Kizuna Encounter: Super Tag Battle"]),
    ("hl", &["Hellblade: Senua's Sacrifice"]),
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
    ("doom", "DOOM"),
    ("kerbel", "Kerbal Space Program"),
    ("hollow kinght", "Hollow Knight"),
    ("stardwe", "Stardew Valley"),
    ("celeset", "Celeste"),
    ("portla", "Portal"),
    ("cyberpnuk", "Cyberpunk 2077"),
    ("okami", "Ōkami HD"),
    ("tr", "Tomb Raider"),
    ("ksp", "Kerbal Space Program"),
    ("p2", "Portal 2"),
    ("xcs", "XCOM®: Chimera Squad"),
    ("ttr", "Rise of the Tomb Raider"),
    ("bg3", "Baldur's Gate 3"),
];

/// (query, titles that must all appear, titles they must outrank when present)
const RANKS_ABOVE: &[(&str, &[&str], &[&str])] = &[
    (
        "gta",
        &[
            "Grand Theft Auto V",
            "Grand Theft Auto V Enhanced",
            "Grand Theft Auto IV: The Complete Edition",
            "Grand Theft Auto: Vice City",
        ],
        &[
            "Defense Grid: The Awakening",
            "Gas Station Simulator",
            "Gravitar: Recharged",
        ],
    ),
    (
        "tr",
        &["Tomb Raider"],
        &[
            "Rise of the Tomb Raider",
            "Shadow of the Tomb Raider: Definitive Edition",
            "Trine 2",
            "Straße Racer",
            "Schnellfahrstrecke Köln-Aachen",
        ],
    ),
    (
        "hl",
        &["Half-Life 2", "Half-Life Deathmatch: Source"],
        &["Hollow Knight"],
    ),
];

/// (query, the exact result set) for letters that only match once folded to
/// plain ASCII. Queries are three letters, which get no typo budget, so a
/// pass means the fold produced a literal match rather than a lucky typo.
const FOLDS_TO_EXACTLY: &[(&str, &[&str])] = &[
    ("oka", &["Ōkami HD"]),
    ("swi", &["ŚWIAT Racer"]),
    ("zol", &["Żółw Ninja"]),
    ("lod", &["Łódź Story"]),
    ("ors", &["Ørsted Islands"]),
    ("dor", &["Đorđe Adventure"]),
    ("aeo", &["Æon Flux"]),
    ("sse", &["Straße Racer"]),
    ("oeu", &["Œuvre Collection"]),
    ("brü", &["Brütal Legend"]),
];

#[test]
fn folds_accented_and_special_letters() {
    let library = library();
    let mut failures = Vec::new();
    for (query, expected) in FOLDS_TO_EXACTLY {
        let names = result_names(query, &library);
        if names != *expected {
            failures.push(format!(
                "{query:?} should return exactly {expected:?}, got {names:?}"
            ));
        }
    }
    assert_no_failures(failures);
}

#[test]
fn query_of_only_combining_marks_acts_like_blank() {
    let library = library();
    let results = search::fuzzy_search("\u{301}", &library);
    assert_eq!(results.len(), library.len());
    assert!(results.iter().all(|r| r.score == 0));
}

/// (query, title, exact char positions that must be highlighted)
const HIGHLIGHTS: &[(&str, &str, &[u32])] = &[
    ("celeste", "Celeste", &[0, 1, 2, 3, 4, 5, 6]),
    ("brutal", "Brütal Legend", &[0, 1, 2, 3, 4, 5]),
    ("legend", "Brütal Legend", &[7, 8, 9, 10, 11, 12]),
    (
        "knight",
        "Batman™: Arkham Knight",
        &[16, 17, 18, 19, 20, 21],
    ),
    ("okami", "Ōkami HD", &[0, 1, 2, 3, 4]),
    ("strasse", "Straße Racer", &[0, 1, 2, 3, 4, 5]),
    (
        "tomb raider",
        "Tomb Raider",
        &[0, 1, 2, 3, 5, 6, 7, 8, 9, 10],
    ),
    ("gta", "Grand Theft Auto V", &[0, 6, 12]),
    ("gta", "Defense Grid: The Awakening", &[8, 14, 18]),
    ("tr", "Tomb Raider", &[0, 5]),
    ("tr", "Rise of the Tomb Raider", &[12, 17]),
    ("ak", "Batman™: Arkham Knight", &[9, 16]),
    ("xcs", "XCOM®: Chimera Squad", &[0, 7, 15]),
    ("oh", "Ōkami HD", &[0, 6]),
    ("dga", "Death's Gambit - Afterlife", &[0, 8, 17]),
    ("ac", "Assassin’s Creed", &[0, 11]),
    ("p2", "Portal 2", &[0, 7]),
    ("ae", "Æon Flux", &[0]),
];

/// Every query used by any table, for invariant checks that apply to all of them.
fn all_queries() -> Vec<&'static str> {
    MUST_FIND
        .iter()
        .chain(MUST_NOT_FIND)
        .chain(FOLDS_TO_EXACTLY)
        .map(|(q, _)| *q)
        .chain(RANKS_FIRST.iter().map(|(q, _)| *q))
        .chain(RANKS_ABOVE.iter().map(|(q, _, _)| *q))
        .chain(HIGHLIGHTS.iter().map(|(q, _, _)| *q))
        .collect()
}

#[test]
fn highlights_exact_characters() {
    let library = library();
    let mut failures = Vec::new();
    for (query, title, expected) in HIGHLIGHTS {
        let results = search::fuzzy_search(query, &library);
        match results.iter().find(|r| r.game.name == *title) {
            Some(r) if r.match_indices == *expected => {}
            Some(r) => failures.push(format!(
                "{query:?} on {title:?} should highlight {expected:?}, got {:?}",
                r.match_indices
            )),
            None => failures.push(format!("{query:?} should find {title:?}")),
        }
    }
    assert_no_failures(failures);
}

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
    let mut failures = Vec::new();
    for query in all_queries() {
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

#[test]
fn ranks_titles_above_others() {
    let library = library();
    let mut failures = Vec::new();
    for (query, above, below) in RANKS_ABOVE {
        let names = result_names(query, &library);
        let rank = |title: &str| names.iter().position(|n| *n == title);
        let mut worst = None;
        for title in *above {
            match rank(title) {
                Some(position) => worst = worst.max(Some(position)),
                None => failures.push(format!("{query:?} should find {title:?}, got {names:?}")),
            }
        }
        for title in *below {
            if let (Some(worst), Some(position)) = (worst, rank(title))
                && position < worst
            {
                failures.push(format!(
                    "{query:?} should rank {above:?} above {title:?}, got {names:?}"
                ));
            }
        }
    }
    assert_no_failures(failures);
}

#[test]
fn no_duplicate_results() {
    let library = library();
    let mut failures = Vec::new();
    for query in all_queries() {
        let mut names = result_names(query, &library);
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        if names.len() != total {
            failures.push(format!("{query:?} returned a title more than once"));
        }
    }
    assert_no_failures(failures);
}
