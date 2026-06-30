# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

When the user refers to an "issue" (e.g. "implement issue 5"), it always means a GitHub issue. Look it up with `gh issue view <number>`.

## What this is

`backlog` is a terminal tool that fuzzy-searches a user's game library aggregated across Steam, Epic, GOG, and Amazon. It is a Rust rewrite of a prior Python tool (`own`); some structs note that they match "the Python version's JSON format exactly" for config/cache compatibility. Currently macOS / Apple Silicon focused.

## Commands

```sh
cargo build                      # build
cargo test                       # run all unit + integration tests
cargo test --test sync_test      # run one integration test file (in tests/)
cargo test test_oneshot_output   # run tests matching a name substring
cargo clippy --all-targets       # lint
cargo fmt                        # format

cargo run -- search "hollow knight"   # one-shot search
cargo run -- sync                     # refresh all caches
cargo run -- setup                    # configure Steam credentials
cargo run                             # launch interactive TUI
cargo install --path .                # install the `backlog` binary
```

Edition is **2024** (requires a recent stable toolchain).

## Architecture

The binary (`main.rs`) is a thin CLI dispatcher over a library crate (`lib.rs`). All real logic lives in library modules and is exercised by integration tests in `tests/`, so prefer adding logic to the library rather than `main.rs`.

Data flows in two stages, decoupled by an on-disk cache so that searching never hits the network:

1. **Sync** (`sync.rs`) pulls from sources and writes one JSON cache file per platform into `~/.cache/backlog/<platform>.json`.
2. **Load + search** (`loader.rs` + `search.rs`) reads all cache files, merges them, and fuzzy-matches. The TUI and one-shot `search` both consume this.

The single shared domain type is `Game { name, platform }` (`lib.rs`).

### Sources (`sources/`)
Each source converts an external format into `Vec<Game>`. Two kinds:
- **`steam.rs`** — live Steam Web API calls via `ureq`. Handles owned-games fetch, vanity-URL resolution, and `parse_steam_input` (accepts profile URLs, vanity names, or numeric IDs). Needs an API key + Steam ID from config.
- **`heroic.rs`** — reads the local Heroic launcher's `store_cache/*.json` files (`legendary_library.json` = Epic, `gog_library.json` = GOG with `gog-redist` filtered out, `nile_library.json` = Amazon). No network.

### Cache + config (`cache.rs`, `config.rs`)
- Cache files carry `last_updated` (RFC3339), `source`, and `games`. Caches older than **7 days** are flagged stale by the loader (surfaced as warnings, not errors). Reads of missing/malformed files return `None` and are silently skipped, never panic.
- Config lives at `~/.config/backlog/config.json` (`steam_api_key`, `steam_id`).
- Both modules resolve paths from `$HOME`, falling back to `.` when unset — this fallback is what makes them testable with `tempfile`.

### Search (`search.rs`)
Uses `nucleo-matcher`. Multi-word queries split into atoms that must all match. Results sort by score descending, then a relative threshold (`best * 3/5`) prunes weak matches. `match_indices` are returned for TUI highlighting.

### TUI (`tui.rs`)
`ratatui` + `crossterm`. `run_with(LoadResult)` is the entry point used by `main.rs` (it loads the cache first, syncing on first run when no cache exists). Shows live fuzzy results as the user types, plus a human-readable "synced Nd ago" age from `loader::format_sync_age`.

## Conventions

- **Errors over panics in library code.** Source/cache/config readers return `Option`/`Result` and degrade gracefully (skip a platform, emit a warning) rather than aborting. `main.rs` is where user-facing messages and `process::exit` live.
- **Status reporting via enums.** Sync results use `SyncStatus::{Ok, Skipped, Error}` with a per-platform `SyncReport` rather than bubbling errors up — one failing source must not stop the others.
- Tests are integration-style in `tests/`, one file per module, driving the public library API with `tempfile` for filesystem isolation. Add coverage there when changing library behavior.
