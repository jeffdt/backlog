use backlog::LibraryEntry;
use backlog::queue::Filter;
use backlog::queue::{self, GameState, Queue, QueueEntry};
use tempfile::TempDir;

fn entry(name: &str, queued: bool, played: bool) -> QueueEntry {
    QueueEntry {
        key: queue::normalize_key(name),
        name: name.to_string(),
        queued,
        played,
    }
}

#[test]
fn normalize_key_lowercases_and_trims() {
    assert_eq!(queue::normalize_key("  Hollow Knight "), "hollow knight");
    assert_eq!(queue::normalize_key("TUNIC"), "tunic");
}

#[test]
fn load_returns_empty_queue_when_file_missing() {
    let dir = TempDir::new().unwrap();
    let loaded = queue::load(&dir.path().join("nope.json")).unwrap();
    assert!(loaded.entries.is_empty());
}

#[test]
fn load_errors_when_file_is_malformed_rather_than_reporting_an_empty_queue() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("queue.json");
    std::fs::write(&path, "{ not json at all").unwrap();
    let err = queue::load(&path).unwrap_err();
    assert!(err.contains("cannot parse"), "unexpected error: {err}");
    assert!(err.contains("queue.json"), "unexpected error: {err}");
}

#[test]
fn load_errors_when_the_file_cannot_be_read() {
    let dir = TempDir::new().unwrap();
    // A directory in the queue file's place fails to read for a reason other
    // than absence, which must not be mistaken for an empty queue.
    let path = dir.path().join("queue.json");
    std::fs::create_dir(&path).unwrap();
    let err = queue::load(&path).unwrap_err();
    assert!(err.contains("cannot read"), "unexpected error: {err}");
}

#[test]
fn load_refuses_a_file_written_by_a_newer_backlog() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("queue.json");
    std::fs::write(
        &path,
        r#"{"version": 2, "entries": [], "ratings": {"tunic": 5}}"#,
    )
    .unwrap();
    let err = queue::load(&path).unwrap_err();
    assert!(err.contains("schema version 2"), "unexpected error: {err}");
    assert!(
        err.contains("refusing to overwrite"),
        "unexpected error: {err}"
    );
}

#[test]
fn save_leaves_no_temp_file_behind_when_the_rename_fails() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("queue.json");
    // A directory at the destination makes the rename fail while the temp
    // write succeeds.
    std::fs::create_dir(&path).unwrap();
    assert!(queue::save(&path, &Queue::default()).is_err());
    assert!(!dir.path().join("queue.json.tmp").exists());
}

#[test]
fn save_and_load_round_trips_order_and_flags() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("queue.json");
    let q = Queue {
        entries: vec![
            entry("Disco Elysium", true, false),
            entry("Tunic", true, true),
        ],
        ..Default::default()
    };
    queue::save(&path, &q).unwrap();

    let loaded = queue::load(&path).unwrap();
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.entries[0].name, "Disco Elysium");
    assert_eq!(loaded.entries[1].name, "Tunic");
    assert!(loaded.entries[1].played);
}

#[test]
fn save_drops_entries_with_both_flags_cleared() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("queue.json");
    let q = Queue {
        entries: vec![
            entry("Tunic", false, false),
            entry("Pentiment", true, false),
        ],
        ..Default::default()
    };
    queue::save(&path, &q).unwrap();

    let loaded = queue::load(&path).unwrap();
    assert_eq!(loaded.entries.len(), 1);
    assert_eq!(loaded.entries[0].name, "Pentiment");
}

#[test]
fn state_matches_regardless_of_case_and_whitespace() {
    let q = Queue {
        entries: vec![entry("Hollow Knight", true, false)],
        ..Default::default()
    };
    assert_eq!(
        q.state("  hollow knight "),
        GameState {
            queued: true,
            played: false
        }
    );
    assert_eq!(q.state("Unknown Game"), GameState::default());
}

#[test]
fn toggle_queued_and_played_are_independent() {
    let mut q = Queue::default();
    q.toggle_queued("Tunic");
    assert_eq!(
        q.state("Tunic"),
        GameState {
            queued: true,
            played: false
        }
    );

    q.toggle_played("Tunic");
    assert_eq!(
        q.state("Tunic"),
        GameState {
            queued: true,
            played: true
        }
    );

    q.toggle_queued("Tunic");
    assert_eq!(
        q.state("Tunic"),
        GameState {
            queued: false,
            played: true
        }
    );
}

#[test]
fn toggling_both_flags_off_leaves_no_live_state() {
    let mut q = Queue::default();
    q.toggle_queued("Tunic");
    q.toggle_queued("Tunic");
    assert_eq!(q.state("Tunic"), GameState::default());
}

fn queued(names: &[&str]) -> Queue {
    let mut q = Queue::default();
    for name in names {
        q.toggle_queued(name);
    }
    q
}

#[test]
fn rank_is_one_based_position_among_queued_entries() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds", "Tunic"]);
    q.toggle_played("Pentiment");
    assert_eq!(q.rank("Disco Elysium"), Some(1));
    assert_eq!(q.rank("Outer Wilds"), Some(2));
    assert_eq!(q.rank("tunic"), Some(3));
    assert_eq!(q.rank("Pentiment"), None);
    assert_eq!(q.rank("Not In Queue"), None);
}

#[test]
fn newly_queued_game_takes_the_last_rank() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds"]);
    q.toggle_queued("Tunic");
    assert_eq!(
        q.queued_names(),
        vec!["Disco Elysium", "Outer Wilds", "Tunic"]
    );
}

#[test]
fn requeuing_a_played_game_sends_it_to_the_back() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds"]);
    q.toggle_played("Disco Elysium");
    q.toggle_queued("Disco Elysium");
    assert_eq!(q.queued_names(), vec!["Outer Wilds"]);
    q.toggle_queued("Disco Elysium");
    assert_eq!(q.queued_names(), vec!["Outer Wilds", "Disco Elysium"]);
}

#[test]
fn move_down_swaps_with_the_next_queued_game() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds", "Tunic"]);
    assert!(q.move_down("Disco Elysium"));
    assert_eq!(
        q.queued_names(),
        vec!["Outer Wilds", "Disco Elysium", "Tunic"]
    );
}

#[test]
fn move_up_swaps_with_the_previous_queued_game() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds", "Tunic"]);
    assert!(q.move_up("Tunic"));
    assert_eq!(
        q.queued_names(),
        vec!["Disco Elysium", "Tunic", "Outer Wilds"]
    );
}

#[test]
fn moves_are_no_ops_at_the_boundaries() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds"]);
    assert!(!q.move_up("Disco Elysium"));
    assert!(!q.move_down("Outer Wilds"));
    assert_eq!(q.queued_names(), vec!["Disco Elysium", "Outer Wilds"]);
}

#[test]
fn moves_are_no_ops_for_games_that_are_not_queued() {
    let mut q = queued(&["Disco Elysium", "Outer Wilds"]);
    q.toggle_played("Pentiment");
    assert!(!q.move_up("Pentiment"));
    assert!(!q.move_down("Pentiment"));
    assert!(!q.move_up("Never Seen"));
}

#[test]
fn move_down_skips_over_played_only_entries() {
    let mut q = Queue::default();
    q.toggle_queued("Disco Elysium");
    q.toggle_played("Pentiment");
    q.toggle_queued("Outer Wilds");
    assert!(q.move_down("Disco Elysium"));
    assert_eq!(q.queued_names(), vec!["Outer Wilds", "Disco Elysium"]);
}

#[test]
fn move_up_skips_over_played_only_entries() {
    let mut q = Queue::default();
    q.toggle_queued("Disco Elysium");
    q.toggle_played("Pentiment");
    q.toggle_queued("Outer Wilds");
    assert!(q.move_up("Outer Wilds"));
    assert_eq!(q.queued_names(), vec!["Outer Wilds", "Disco Elysium"]);
}

fn library(names: &[&str]) -> Vec<LibraryEntry> {
    names
        .iter()
        .map(|n| LibraryEntry {
            name: n.to_string(),
            platforms: vec!["steam".to_string()],
        })
        .collect()
}

fn names(entries: &[LibraryEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn filter_cycles_forward_and_backward() {
    assert_eq!(Filter::All.next(), Filter::Queued);
    assert_eq!(Filter::Queued.next(), Filter::Played);
    assert_eq!(Filter::Played.next(), Filter::Unplayed);
    assert_eq!(Filter::Unplayed.next(), Filter::All);

    assert_eq!(Filter::All.prev(), Filter::Unplayed);
    assert_eq!(Filter::Unplayed.prev(), Filter::Played);
    assert_eq!(Filter::Played.prev(), Filter::Queued);
    assert_eq!(Filter::Queued.prev(), Filter::All);
}

#[test]
fn filter_all_returns_the_whole_library_in_order() {
    let lib = library(&["Tunic", "Outer Wilds", "Pentiment"]);
    let q = Queue::default();
    let out = queue::apply_filter(&lib, &q, Filter::All);
    assert_eq!(names(&out), vec!["Tunic", "Outer Wilds", "Pentiment"]);
}

#[test]
fn filter_queued_returns_games_in_rank_order_not_library_order() {
    let lib = library(&["Tunic", "Outer Wilds", "Pentiment"]);
    let mut q = Queue::default();
    q.toggle_queued("Pentiment");
    q.toggle_queued("Tunic");
    let out = queue::apply_filter(&lib, &q, Filter::Queued);
    assert_eq!(names(&out), vec!["Pentiment", "Tunic"]);
}

#[test]
fn filter_queued_skips_games_missing_from_the_library() {
    let lib = library(&["Tunic"]);
    let mut q = Queue::default();
    q.toggle_queued("Uninstalled Game");
    q.toggle_queued("Tunic");
    let out = queue::apply_filter(&lib, &q, Filter::Queued);
    assert_eq!(names(&out), vec!["Tunic"]);
}

#[test]
fn filter_played_returns_only_played_games_in_library_order() {
    let lib = library(&["Tunic", "Outer Wilds", "Pentiment"]);
    let mut q = Queue::default();
    q.toggle_played("Pentiment");
    q.toggle_played("Tunic");
    let out = queue::apply_filter(&lib, &q, Filter::Played);
    assert_eq!(names(&out), vec!["Tunic", "Pentiment"]);
}

#[test]
fn filter_unplayed_includes_queued_but_unplayed_games() {
    let lib = library(&["Tunic", "Outer Wilds", "Pentiment"]);
    let mut q = Queue::default();
    q.toggle_played("Pentiment");
    q.toggle_queued("Tunic");
    let out = queue::apply_filter(&lib, &q, Filter::Unplayed);
    assert_eq!(names(&out), vec!["Tunic", "Outer Wilds"]);
}
