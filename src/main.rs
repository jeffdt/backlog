use std::io::{self, Write};

use clap::{Parser, Subcommand};

use bouncer::{cache, config, itad, loader, output, queue, search, sources, sync};

#[derive(Parser)]
#[command(
    name = "bouncer",
    about = "Search and queue your game library across Steam, Epic, GOG, and Amazon"
)]
struct Cli {
    /// Game name to search for (opens TUI if omitted)
    query: Option<String>,

    // Declared by hand rather than via `#[command(version)]`, which always
    // prefixes its output with the binary name.
    /// Print version and exit
    #[arg(short = 'V', long)]
    version: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Search your library (same as `bouncer <query>`)
    Search {
        /// Game name to search for
        query: String,
    },
    /// Refresh caches from sources
    Sync,
    /// Configure Steam and IsThereAnyDeal API credentials
    Setup,
    /// Print the ranked queue
    Queue,
    /// Check prices on IsThereAnyDeal
    #[command(long_about = PRICE_LONG_ABOUT)]
    Price {
        /// Print results as JSON
        #[arg(long)]
        json: bool,
        /// Game titles, Steam store URLs, Steam app IDs, or IsThereAnyDeal game IDs
        #[arg(required = true)]
        inputs: Vec<String>,
    },
}

const PRICE_LONG_ABOUT: &str = r#"Look up current and historical prices on IsThereAnyDeal.

Each input is a game title, a Steam store URL, a Steam app ID, or an
IsThereAnyDeal game ID. Titles that match exactly print one row; titles
that don't print up to 5 priced candidates, so pick the one you meant.

Columns: Steam's current price, the best current price across stores
(with the store, which may be a key reseller or another launcher), and
the all-time low (with store and date).

Checking a Steam cart:
  1. For each game, run `bouncer search "<title>"` to see if you already own it.
     A match on any platform, Steam included, means you may not need it.
  2. Run one `bouncer price "<title>" "<title>" ...` for the rest.
  For bundles that "include 1 item", price the included game, and compare
  against the cart price: Steam's complete-the-set discount is not in
  IsThereAnyDeal's Steam price.

Exits 1 if any input could not be found."#;

fn main() {
    let cli = Cli::parse();

    if cli.version {
        writeln!(io::stdout(), "{}", env!("CARGO_PKG_VERSION")).ok();
        return;
    }

    match cli.command {
        Some(Commands::Search { query }) => run_search(&query),
        Some(Commands::Setup) => run_setup(),
        Some(Commands::Sync) => run_sync(),
        Some(Commands::Queue) => run_queue(),
        Some(Commands::Price { json, inputs }) => run_price(&inputs, json),
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
        eprintln!("No games loaded. Run `bouncer sync` first.");
        return;
    }

    let matches = search::fuzzy_search(query, &result.games);

    if matches.is_empty() {
        writeln!(io::stdout(), "No matches for '{query}'.").ok();
        return;
    }

    let q = load_queue_or_warn();
    writeln!(io::stdout(), "{}", output::format_search_rows(&matches, &q)).ok();
}

fn run_queue() {
    let cache_dir = cache::default_cache_dir();
    let result = loader::load_all_games(&cache_dir);

    for w in &result.warnings {
        eprintln!("Warning: {w}");
    }

    if result.games.is_empty() {
        eprintln!("No games loaded. Run `bouncer sync` first.");
        return;
    }

    let q = load_queue_or_warn();
    let rows = output::format_queue_rows(&result.games, &q);

    if rows.is_empty() {
        eprintln!("Queue is empty. Press Enter on a game in the TUI to queue it.");
        return;
    }
    writeln!(io::stdout(), "{rows}").ok();
}

fn run_price(inputs: &[String], json: bool) {
    if inputs.iter().any(|i| i.trim().is_empty()) {
        eprintln!("Error: empty input");
        std::process::exit(1);
    }
    let key = config::load_config(&config::default_config_path())
        .and_then(|c| c.itad_api_key)
        .filter(|k| !k.is_empty());
    let Some(key) = key else {
        eprintln!("No IsThereAnyDeal API key. Run `bouncer setup` to add one.");
        std::process::exit(1);
    };
    let groups = itad::check_prices(&key, inputs).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    if json {
        let text = serde_json::to_string_pretty(&groups).expect("price groups serialize");
        writeln!(io::stdout(), "{text}").ok();
    } else {
        let table = output::format_price_groups(&groups);
        if !table.is_empty() {
            writeln!(io::stdout(), "{table}").ok();
        }
    }

    let missing = groups.iter().filter(|g| g.kind == itad::MatchKind::None);
    if missing
        .inspect(|g| eprintln!("not found: {}", g.input))
        .count()
        > 0
    {
        std::process::exit(1);
    }
}

/// Loads queue state for a read-only command, warning and continuing with an
/// empty queue when the file cannot be read.
fn load_queue_or_warn() -> queue::Queue {
    queue::load(&queue::default_queue_path()).unwrap_or_else(|e| {
        eprintln!("Warning: {e}");
        queue::Queue::default()
    })
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
    let existing_itad_key = config::load_config(&config_path).and_then(|c| c.itad_api_key);
    eprintln!("Steam API setup");
    eprintln!("  Get your API key at: https://steamcommunity.com/dev/apikey");

    let api_key = prompt("  Steam API key: ");
    let input = prompt("  Steam profile URL or vanity name: ");

    eprint!("  Resolving Steam ID...");
    io::stderr().flush().ok();
    let steam_id = match sources::steam::parse_steam_input(&api_key, &input) {
        Ok(id) => {
            eprintln!(" {id}");
            id
        }
        Err(e) => {
            eprintln!("\n  Error: {e}");
            eprintln!("Setup failed, keeping existing config.");
            std::process::exit(1);
        }
    };

    let itad_prompt = if existing_itad_key.is_some() {
        "  IsThereAnyDeal API key (Enter to keep current): "
    } else {
        "  IsThereAnyDeal API key (Enter to skip, get one at https://isthereanydeal.com/apps/my/): "
    };
    let itad_input = prompt(itad_prompt);
    let itad_api_key = if itad_input.is_empty() {
        existing_itad_key
    } else {
        Some(itad_input)
    };

    let cfg = config::Config {
        steam_api_key: api_key,
        steam_id,
        itad_api_key,
    };
    config::save_config(&config_path, &cfg).expect("Failed to save config");
    eprintln!("Config saved to {}", config_path.display());
}

/// Prompts the user via stderr and reads a line from stdin.
fn prompt(message: &str) -> String {
    eprint!("{message}");
    io::stderr().flush().ok();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

fn run_tui() {
    let cache_dir = cache::default_cache_dir();
    let heroic_dir = sources::heroic::heroic_store_cache_dir();
    let config_path = config::default_config_path();
    let load_result = loader::load_all_games(&cache_dir);

    let load_result = if load_result.games.is_empty() {
        eprintln!("No cached games found, syncing...");
        run_sync();
        loader::load_all_games(&cache_dir)
    } else {
        load_result
    };

    let queue_path = queue::default_queue_path();
    if let Err(e) =
        bouncer::tui::run_with(load_result, cache_dir, heroic_dir, config_path, queue_path)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
