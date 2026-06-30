use backlog::{LibraryEntry, search};

fn sample_library() -> Vec<LibraryEntry> {
    vec![
        LibraryEntry {
            name: "Tomb Raider".to_string(),
            platforms: vec!["steam".to_string()],
        },
        LibraryEntry {
            name: "Tomb Raider I".to_string(),
            platforms: vec!["steam".to_string()],
        },
        LibraryEntry {
            name: "Tomb Raider II".to_string(),
            platforms: vec!["steam".to_string()],
        },
        LibraryEntry {
            name: "Rise of the Tomb Raider".to_string(),
            platforms: vec!["epic".to_string()],
        },
        LibraryEntry {
            name: "Celeste".to_string(),
            platforms: vec!["epic".to_string()],
        },
        LibraryEntry {
            name: "Hollow Knight".to_string(),
            platforms: vec!["gog".to_string()],
        },
        LibraryEntry {
            name: "TOEM".to_string(),
            platforms: vec!["epic".to_string()],
        },
        LibraryEntry {
            name: "Braid".to_string(),
            platforms: vec!["gog".to_string()],
        },
        LibraryEntry {
            name: "Hades".to_string(),
            platforms: vec!["epic".to_string()],
        },
    ]
}

#[test]
fn test_tomb_raider_search() {
    let library = sample_library();
    let results = search::fuzzy_search("tomb raider", &library);
    assert!(!results.is_empty());
    // All Tomb Raider variants should appear
    let names: Vec<&str> = results.iter().map(|r| r.game.name.as_str()).collect();
    assert!(names.contains(&"Tomb Raider"));
    assert!(names.contains(&"Rise of the Tomb Raider"));
    // TOEM should NOT appear (nucleo matches chars in sequence, not substring similarity)
    assert!(!names.contains(&"TOEM"));
    // Braid should NOT appear
    assert!(!names.contains(&"Braid"));
}

#[test]
fn test_tomb_search_finds_tomb_raider() {
    let library = sample_library();
    let results = search::fuzzy_search("tomb", &library);
    let names: Vec<&str> = results.iter().map(|r| r.game.name.as_str()).collect();
    assert!(names.contains(&"Tomb Raider"));
}

#[test]
fn test_search_results_are_ranked_by_score() {
    let library = sample_library();
    let results = search::fuzzy_search("tomb raider", &library);
    // Exact match "Tomb Raider" should rank first or very high
    assert_eq!(results[0].game.name, "Tomb Raider");
    // Scores should be in descending order
    for window in results.windows(2) {
        assert!(window[0].score >= window[1].score);
    }
}

#[test]
fn test_search_empty_query() {
    let library = sample_library();
    let results = search::fuzzy_search("", &library);
    // Empty query returns all games (nucleo returns everything with score 0)
    assert_eq!(results.len(), library.len());
}

#[test]
fn test_search_no_matches() {
    let library = sample_library();
    let results = search::fuzzy_search("zzzznotarealgame", &library);
    assert!(results.is_empty());
}

#[test]
fn test_search_empty_library() {
    let results = search::fuzzy_search("test", &[]);
    assert!(results.is_empty());
}

#[test]
fn test_search_case_insensitive() {
    let library = sample_library();
    let results = search::fuzzy_search("CELESTE", &library);
    let names: Vec<&str> = results.iter().map(|r| r.game.name.as_str()).collect();
    assert!(names.contains(&"Celeste"));
}

#[test]
fn test_match_indices_populated() {
    let library = sample_library();
    let results = search::fuzzy_search("celeste", &library);
    let celeste = results.iter().find(|r| r.game.name == "Celeste").unwrap();
    assert!(!celeste.match_indices.is_empty());
}

#[test]
fn test_cross_store_dupe_yields_single_result() {
    let library = vec![LibraryEntry {
        name: "Card Shark".to_string(),
        platforms: vec!["epic".to_string(), "steam".to_string()],
    }];
    let results = search::fuzzy_search("card shark", &library);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].game.platforms, vec!["epic", "steam"]);
}
