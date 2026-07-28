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
    let loaded = queue::load(&dir.path().join("nope.json"));
    assert!(loaded.entries.is_empty());
}

#[test]
fn load_returns_empty_queue_when_file_malformed() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("queue.json");
    std::fs::write(&path, "{ not json at all").unwrap();
    let loaded = queue::load(&path);
    assert!(loaded.entries.is_empty());
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

    let loaded = queue::load(&path);
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

    let loaded = queue::load(&path);
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
