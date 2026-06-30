use backlog::{LibraryEntry, search};

fn format_results(results: &[search::SearchResult]) -> String {
    if results.is_empty() {
        return String::new();
    }
    let max_name_len = results.iter().map(|r| r.game.name.len()).max().unwrap_or(0);
    results
        .iter()
        .map(|r| {
            let platform_label = r.game.platforms.join(" / ");
            format!(
                "{:<width$}  {}",
                r.game.name,
                platform_label,
                width = max_name_len
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_oneshot_output_format() {
    let entries = [
        LibraryEntry {
            name: "Tomb Raider".to_string(),
            platforms: vec!["steam".to_string()],
        },
        LibraryEntry {
            name: "Rise of the Tomb Raider".to_string(),
            platforms: vec!["epic".to_string()],
        },
    ];
    let results: Vec<search::SearchResult> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| search::SearchResult {
            game: e,
            score: (100 - i as u32),
            match_indices: Vec::new(),
        })
        .collect();
    let output = format_results(&results);
    assert!(output.contains("Tomb Raider"));
    assert!(output.contains("steam"));
    // Columns should be aligned
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    // Platform column should start at the same position
    let steam_pos = lines[0].find("steam").unwrap();
    let epic_pos = lines[1].find("epic").unwrap();
    assert_eq!(steam_pos, epic_pos);
}

#[test]
fn test_oneshot_output_empty() {
    let results: Vec<search::SearchResult> = Vec::new();
    let output = format_results(&results);
    assert!(output.is_empty());
}

#[test]
fn test_oneshot_output_single() {
    let entries = [LibraryEntry {
        name: "Hades".to_string(),
        platforms: vec!["epic".to_string()],
    }];
    let results: Vec<search::SearchResult> = entries
        .iter()
        .map(|e| search::SearchResult {
            game: e,
            score: 100,
            match_indices: Vec::new(),
        })
        .collect();
    let output = format_results(&results);
    assert_eq!(output, "Hades  epic");
}

#[test]
fn test_oneshot_output_combined_platform_label() {
    let entries = [LibraryEntry {
        name: "Card Shark".to_string(),
        platforms: vec!["epic".to_string(), "steam".to_string()],
    }];
    let results: Vec<search::SearchResult> = entries
        .iter()
        .map(|e| search::SearchResult {
            game: e,
            score: 100,
            match_indices: Vec::new(),
        })
        .collect();
    let output = format_results(&results);
    // Must be a single line with the combined label
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("epic / steam"));
}
