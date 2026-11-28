use std::process::Command;

use bouncer::queue::Queue;
use bouncer::{LibraryEntry, output, search};

fn library(names: &[&str]) -> Vec<LibraryEntry> {
    names
        .iter()
        .map(|n| LibraryEntry {
            name: n.to_string(),
            platforms: vec!["steam".to_string()],
            ..Default::default()
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

/// `release.sh cut` ends on `bouncer --version`, so a missing version flag
/// fails a release after the tag and tap have already been pushed.
#[test]
fn version_flag_prints_the_bare_package_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_bouncer"))
            .arg(flag)
            .output()
            .expect("failed to run the bouncer binary");

        assert!(out.status.success(), "{flag} exited with {}", out.status);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            env!("CARGO_PKG_VERSION")
        );
    }
}

#[test]
fn price_without_key_exits_with_setup_hint() {
    let home = tempfile::TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_bouncer"))
        .env("HOME", home.path())
        .args(["price", "Hades"])
        .output()
        .expect("failed to run the bouncer binary");

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bouncer setup"));
}

#[test]
fn price_rejects_blank_input_before_any_network_call() {
    let home = tempfile::TempDir::new().unwrap();
    let config_dir = home.path().join(".config/bouncer");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"steam_api_key": "k", "steam_id": "1", "itad_api_key": "dummy"}"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_bouncer"))
        .env("HOME", home.path())
        .args(["price", "  "])
        .output()
        .expect("failed to run the bouncer binary");

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("empty input"));
}

/// `search` is the explicit form of the bare query, so a title that collides
/// with a subcommand name can still be searched.
#[test]
fn search_subcommand_searches_even_when_the_query_is_a_command_name() {
    let home = tempfile::TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_bouncer"))
        .env("HOME", home.path())
        .args(["search", "price"])
        .output()
        .expect("failed to run the bouncer binary");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("No games loaded"), "stderr: {stderr}");
}

const EXPIRED_TOKEN: &str = "eyAidHlwIjogIkpXVCIgfQ.eyJleHAiOjE3MDAwMDAwMDB9.sig";

fn run_cart_with_stdin(home: &std::path::Path, stdin: &str) -> std::process::Output {
    run_cart(home, None, stdin)
}

fn run_cart(home: &std::path::Path, env_token: Option<&str>, stdin: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bouncer"));
    cmd.env("HOME", home).env_remove("BOUNCER_STEAM_TOKEN");
    if let Some(token) = env_token {
        cmd.env("BOUNCER_STEAM_TOKEN", token);
    }
    let mut child = cmd
        .arg("cart")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run the bouncer binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn cart_rejects_empty_stdin_before_any_network_call() {
    let home = tempfile::TempDir::new().unwrap();
    let out = run_cart_with_stdin(home.path(), "  \n");

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no token"));
}

#[test]
fn cart_without_itad_key_exits_with_setup_hint_before_using_the_token() {
    let home = tempfile::TempDir::new().unwrap();
    let out = run_cart_with_stdin(home.path(), "token\n");

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bouncer setup"));
}

#[test]
fn cart_reads_token_from_env_var_over_empty_stdin() {
    let home = tempfile::TempDir::new().unwrap();
    let out = run_cart(home.path(), Some("token"), "");

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bouncer setup"));
}

#[test]
fn cart_rejects_expired_env_token_before_any_network_call() {
    let home = tempfile::TempDir::new().unwrap();
    let out = run_cart(home.path(), Some(EXPIRED_TOKEN), "");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("BOUNCER_STEAM_TOKEN") && stderr.contains("expired"),
        "stderr: {stderr}"
    );
}

#[test]
fn cart_rejects_expired_stdin_token_before_any_network_call() {
    let home = tempfile::TempDir::new().unwrap();
    let out = run_cart_with_stdin(home.path(), EXPIRED_TOKEN);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("expired") && !stderr.contains("BOUNCER_STEAM_TOKEN"),
        "stderr: {stderr}"
    );
}

#[test]
fn cart_ignores_blank_env_var_and_reads_stdin() {
    let home = tempfile::TempDir::new().unwrap();
    let out = run_cart(home.path(), Some("  "), "  \n");

    assert!(String::from_utf8_lossy(&out.stderr).contains("no token"));
}
