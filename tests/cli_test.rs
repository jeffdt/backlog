use backlog::queue::Queue;
use backlog::{LibraryEntry, output, search};

fn library(names: &[&str]) -> Vec<LibraryEntry> {
    names
        .iter()
        .map(|n| LibraryEntry {
            name: n.to_string(),
            platforms: vec!["steam".to_string()],
        })
        .collect()
}

fn results(entries: &[LibraryEntry]) -> Vec<search::SearchResult<'_>> {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| search::SearchResult {
            game: e,
            score: 100 - i as u32,
            match_indices: Vec::new(),
        })
        .collect()
}

#[test]
fn search_rows_prefix_markers_for_queued_and_played_games() {
    let lib = library(&[
        "Hollow Knight",
        "Horizon Zero Dawn",
        "Hotline Miami",
        "Ghost of Tsushima",
    ]);
    let mut q = Queue::default();
    q.toggle_queued("Hollow Knight");
    q.toggle_played("Horizon Zero Dawn");
    q.toggle_queued("Hotline Miami");
    q.toggle_played("Hotline Miami");

    let output = output::format_search_rows(&results(&lib), &q);
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].starts_with("» "));
    assert!(lines[1].starts_with(" ✓"));
    assert!(lines[2].starts_with("»✓"));
    assert!(lines[3].starts_with("  "));
}

#[test]
fn search_rows_stay_column_aligned_with_markers() {
    let lib = library(&["Tomb Raider", "Rise of the Tomb Raider"]);
    let mut q = Queue::default();
    q.toggle_queued("Tomb Raider");

    let output = output::format_search_rows(&results(&lib), &q);
    let lines: Vec<&str> = output.lines().collect();
    // Column position, not byte offset: the queued marker (») is a multi-byte
    // UTF-8 character, so a plain `str::find` byte index would diverge from the
    // unqueued row's even though both render as a single terminal column.
    let steam_column = |line: &str| {
        line.find("steam")
            .map(|byte_idx| line[..byte_idx].chars().count())
    };
    assert_eq!(steam_column(lines[0]), steam_column(lines[1]));
}

#[test]
fn queue_rows_are_numbered_in_rank_order() {
    let lib = library(&["Tunic", "Disco Elysium", "Outer Wilds"]);
    let mut q = Queue::default();
    q.toggle_queued("Disco Elysium");
    q.toggle_queued("Outer Wilds");

    let output = output::format_queue_rows(&lib, &q);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].trim_start().starts_with("1"));
    assert!(lines[0].contains("Disco Elysium"));
    assert!(lines[1].trim_start().starts_with("2"));
    assert!(lines[1].contains("Outer Wilds"));
}

#[test]
fn queue_rows_mark_played_games_and_skip_missing_ones() {
    let lib = library(&["Tunic"]);
    let mut q = Queue::default();
    q.toggle_queued("Tunic");
    q.toggle_played("Tunic");
    q.toggle_queued("Uninstalled Game");

    let output = output::format_queue_rows(&lib, &q);
    assert_eq!(output.lines().count(), 1);
    assert!(output.contains('✓'));
}

#[test]
fn queue_rows_keep_the_rank_gap_left_by_a_game_missing_from_the_library() {
    let lib = library(&["Tunic", "Hades"]);
    let mut q = Queue::default();
    q.toggle_queued("Uninstalled Game");
    q.toggle_queued("Tunic");
    q.toggle_queued("Hades");

    let output = output::format_queue_rows(&lib, &q);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].trim_start().starts_with("2 "),
        "got {:?}",
        lines[0]
    );
    assert!(lines[0].contains("Tunic"));
    assert!(
        lines[1].trim_start().starts_with("3 "),
        "got {:?}",
        lines[1]
    );
    assert!(lines[1].contains("Hades"));
}

#[test]
fn rows_join_multiple_platform_labels() {
    let mut lib = library(&["Tunic"]);
    lib[0].platforms = vec!["epic".to_string(), "steam".to_string()];
    let mut q = Queue::default();
    q.toggle_queued("Tunic");

    assert!(output::format_search_rows(&results(&lib), &q).ends_with("epic / steam"));
    assert!(output::format_queue_rows(&lib, &q).ends_with("epic / steam"));
}

#[test]
fn queue_rows_are_empty_when_nothing_is_queued() {
    let lib = library(&["Tunic"]);
    assert!(output::format_queue_rows(&lib, &Queue::default()).is_empty());
}

#[test]
fn search_rows_are_empty_when_there_are_no_results() {
    assert!(output::format_search_rows(&[], &Queue::default()).is_empty());
}
