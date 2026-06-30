use backlog::{Game, search};

fn format_results(results: &[search::SearchResult]) -> String {
    if results.is_empty() {
        return String::new();
    }
    let max_name_len = results.iter().map(|r| r.game.name.len()).max().unwrap_or(0);
    results
        .iter()
        .map(|r| format!("{:<width$}  {}", r.game.name, r.game.platform, width = max_name_len))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_oneshot_output_format() {
    let games = vec![
        Game { name: "Tomb Raider".to_string(), platform: "steam".to_string() },
        Game { name: "Rise of the Tomb Raider".to_string(), platform: "epic".to_string() },
    ];
    let results: Vec<search::SearchResult> = games
        .iter()
        .enumerate()
        .map(|(i, g)| search::SearchResult {
            game: g,
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
    let games = vec![Game { name: "Hades".to_string(), platform: "epic".to_string() }];
    let results: Vec<search::SearchResult> = games
        .iter()
        .map(|g| search::SearchResult { game: g, score: 100, match_indices: Vec::new() })
        .collect();
    let output = format_results(&results);
    assert_eq!(output, "Hades  epic");
}
