use std::collections::{BTreeSet, HashMap};
use std::io::{self, IsTerminal, Read, Write};

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};

use bouncer::{cache, cart, config, itad, loader, output, queue, search, sources, sync};

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
    Sync {
        /// List the titles added and removed on each platform
        #[arg(short, long)]
        verbose: bool,
    },
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
    /// Judge a Steam cart against your library and all-time lows
    #[command(long_about = CART_LONG_ABOUT)]
    Cart {
        /// Print results as JSON
        #[arg(long)]
        json: bool,
    },
}

const PRICE_LONG_ABOUT: &str = r#"Look up current and historical prices on IsThereAnyDeal.

Each input is a game title, a Steam store URL, a Steam app ID, or an
IsThereAnyDeal game ID. Titles that match exactly print one row; titles
that don't print up to 5 priced candidates, so pick the one you meant.

Columns: Steam's current price, the best current price across stores
(with the store, which may be a key reseller or another launcher), and
the all-time low (with store and date).

To check a whole Steam cart at once, see `bouncer cart --help`.

Exits 1 if any input could not be found."#;

const CART_LONG_ABOUT: &str = r#"Judge every item in your Steam cart.

Each item is checked against your synced library (any platform) and its
IsThereAnyDeal price history, then marked:
  at low           at or under the all-time low from any store
  at Steam low     at or under Steam's own low, but another store went lower
  above Steam low  Steam itself has sold it cheaper
  above low        above the all-time low, with no Steam low known
  already owned    you own a game in it on some platform
  nothing new      a bundle whose games you all own on Steam
  no low known     no price history to compare against

Totals follow: the cart, every item at today's cheapest store, at
Steam's lows, and at all-time lows.

Bundles use the cart price, since Steam leaves games you own on Steam
out of a bundle; those games are shown as excluded and do not count.
Packages use the live store price.

Steam has no cart API for Web API keys, so this needs your store
session token, which bouncer never saves:
  1. While logged in, open
     https://store.steampowered.com/pointssummary/ajaxgetasyncconfig
  2. Copy the `webapi_token` value.
  3. export BOUNCER_STEAM_TOKEN=$(pbpaste)
  4. bouncer cart  (as often as you like in that shell)

Or pipe it in for a single run: pbpaste | bouncer cart

The token grants full access to your Steam account until it expires
(about a day). Treat it like a password: don't save it or paste it into
anything you don't trust. Export it with $(pbpaste), never the literal
value, so it stays out of shell history, and note that every program
started from that shell can read it."#;

const TOKEN_ENV_VAR: &str = "BOUNCER_STEAM_TOKEN";

fn main() {
    let cli = Cli::parse();

    if cli.version {
        writeln!(io::stdout(), "{}", env!("CARGO_PKG_VERSION")).ok();
        return;
    }

    match cli.command {
        Some(Commands::Search { query }) => run_search(&query),
        Some(Commands::Setup) => run_setup(),
        Some(Commands::Sync { verbose }) => run_sync(verbose),
        Some(Commands::Queue) => run_queue(),
        Some(Commands::Price { json, inputs }) => run_price(&inputs, json),
        Some(Commands::Cart { json }) => run_cart(json),
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
    let key = itad_key_or_exit();
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

/// Returns the configured IsThereAnyDeal key, exiting with setup advice when absent.
fn itad_key_or_exit() -> String {
    let key = config::load_config(&config::default_config_path())
        .and_then(|c| c.itad_api_key)
        .filter(|k| !k.is_empty());
    key.unwrap_or_else(|| {
        eprintln!("No IsThereAnyDeal API key. Run `bouncer setup` to add one.");
        std::process::exit(1);
    })
}

fn run_cart(json: bool) {
    let fail = |e: String| -> ! {
        eprintln!("Error: {e}");
        std::process::exit(1);
    };
    let env_token = std::env::var(TOKEN_ENV_VAR).unwrap_or_default();
    let from_env = !env_token.trim().is_empty();
    let mut token = env_token;
    if !from_env {
        // Refuse a terminal so the token is never typed where it echoes on screen.
        if io::stdin().is_terminal() {
            fail(format!(
                "export {TOKEN_ENV_VAR}=$(pbpaste) or pipe your Steam store token in. See `bouncer cart --help`."
            ));
        }
        token.clear();
        io::stdin()
            .read_to_string(&mut token)
            .unwrap_or_else(|e| fail(format!("could not read token: {e}")));
    }
    let token = token.trim();
    if token.is_empty() {
        fail("no token on stdin. See `bouncer cart --help`.".to_string());
    }
    if let Some(exp) = cart::token_expiry(token).and_then(|s| DateTime::from_timestamp(s, 0))
        && exp <= Utc::now()
    {
        let age = loader::format_sync_age(Some(exp)).replacen("synced", "expired", 1);
        fail(if from_env {
            format!("the token in {TOKEN_ENV_VAR} {age}; fetch a fresh one and re-export it")
        } else {
            format!("the token {age}; fetch a fresh one")
        });
    }
    let key = itad_key_or_exit();

    let lines = cart::fetch_cart(token).unwrap_or_else(|e| fail(e));
    if lines.is_empty() {
        eprintln!("Cart is empty.");
        return;
    }
    let items = cart::fetch_store_items(&lines).unwrap_or_else(|e| fail(e));
    let appids: BTreeSet<u64> = items
        .values()
        .flat_map(|i| i.apps.iter().map(|(id, _)| *id))
        .collect();
    let inputs: Vec<String> = appids.iter().map(u64::to_string).collect();
    let groups = itad::check_prices(&key, &inputs).unwrap_or_else(|e| fail(e));
    let prices: HashMap<_, _> = appids
        .into_iter()
        .zip(groups)
        .map(|(id, g)| {
            let game = g.games.into_iter().next();
            let prices = game.map_or_else(cart::AppPrices::default, |g| cart::AppPrices {
                steam_low: g.steam.and_then(|s| s.store_low),
                best: g.best,
                low: g.low,
            });
            (id, prices)
        })
        .collect();

    let library = loader::load_all_games(&cache::default_cache_dir());
    for w in &library.warnings {
        eprintln!("Warning: {w}");
    }
    if library.games.is_empty() {
        eprintln!(
            "Warning: no games loaded, so ownership is not checked. Run `bouncer sync` first."
        );
    }

    let rows = cart::assess(&lines, &items, &library.games, &prices);
    if json {
        let report = serde_json::json!({ "items": rows, "totals": cart::totals(&rows) });
        let text = serde_json::to_string_pretty(&report).expect("cart report serializes");
        writeln!(io::stdout(), "{text}").ok();
    } else {
        writeln!(io::stdout(), "{}", output::format_cart_rows(&rows)).ok();
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

fn run_sync(verbose: bool) {
    let heroic_dir = sources::heroic::heroic_store_cache_dir();
    let config_path = config::default_config_path();
    let cache_dir = cache::default_cache_dir();

    eprintln!("Syncing libraries...");
    let reports = sync::sync_all(&heroic_dir, &config_path, &cache_dir);

    for report in &reports {
        match &report.status {
            sync::SyncStatus::Ok => {
                eprintln!(
                    "  {}: {} games (+{} -{})",
                    report.platform,
                    report.game_count,
                    report.added.len(),
                    report.removed.len()
                );
                if verbose {
                    for name in &report.added {
                        eprintln!("    + {name}");
                    }
                    for name in &report.removed {
                        eprintln!("    - {name}");
                    }
                }
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
        run_sync(false);
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
