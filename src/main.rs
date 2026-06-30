use std::io::{self, Write};

use clap::{Parser, Subcommand};

use backlog::{cache, config, loader, search, sources, sync};

#[derive(Parser)]
#[command(name = "backlog", about = "Search your game library across Steam, Epic, GOG, and Amazon")]
struct Cli {
    /// Game name to search for (opens TUI if omitted)
    query: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Refresh caches from sources
    Sync,
    /// Configure Steam API credentials
    Setup,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Setup) => run_setup(),
        Some(Commands::Sync) => run_sync(),
        None => {
            if let Some(query) = cli.query {
                run_search(&query);
            } else {
                run_tui();
            }
        }
    }
}

fn run_search(query: &str) {
    let cache_dir = cache::default_cache_dir();
    let result = loader::load_all_games(&cache_dir);

    for w in &result.warnings {
        eprintln!("Warning: {w}");
    }

    if result.games.is_empty() {
        eprintln!("No games loaded. Run `backlog sync` first.");
        return;
    }

    let matches = search::fuzzy_search(query, &result.games);

    if matches.is_empty() {
        println!("No matches for '{query}'.");
        return;
    }

    let max_name_len = matches.iter().map(|r| r.game.name.len()).max().unwrap_or(0);
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for r in &matches {
        writeln!(out, "{:<width$}  {}", r.game.name, r.game.platform, width = max_name_len).ok();
    }
}

fn run_sync() {
    let heroic_dir = sources::heroic::heroic_store_cache_dir();
    let config_path = config::default_config_path();
    let cache_dir = cache::default_cache_dir();

    eprintln!("Syncing libraries...");
    let reports = sync::sync_all(&heroic_dir, &config_path, &cache_dir);

    for report in &reports {
        match &report.status {
            sync::SyncStatus::Ok => {
                eprintln!("  {}: {} games", report.platform, report.game_count);
            }
            sync::SyncStatus::Skipped(reason) => {
                eprintln!("  {}: skipped ({reason})", report.platform);
            }
            sync::SyncStatus::Error(err) => {
                eprintln!("  {}: error ({err})", report.platform);
            }
        }
    }
    eprintln!("Done.");
}

fn run_setup() {
    let config_path = config::default_config_path();
    eprintln!("Steam API setup");
    eprintln!("  Get your API key at: https://steamcommunity.com/dev/apikey");

    let api_key = prompt("  Steam API key: ");
    let steam_id = prompt("  Steam ID: ");

    let cfg = config::Config { steam_api_key: api_key, steam_id };
    config::save_config(&config_path, &cfg).expect("Failed to save config");
    eprintln!("Config saved to {}", config_path.display());
}

/// Prompts the user via stderr and reads a line from stdin.
fn prompt(message: &str) -> String {
    eprint!("{message}");
    io::stderr().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read input");
    input.trim().to_string()
}

fn run_tui() {
    let cache_dir = cache::default_cache_dir();
    if let Err(e) = backlog::tui::run(&cache_dir) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
