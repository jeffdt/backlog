# AGENTS.md

Orientation for agents and humans working on backlog. This file holds durable
intent and conventions, not a file-by-file map (that goes stale). Read the
source for current structure.

When the user refers to an "issue" (e.g. "implement issue 5"), it always means
a GitHub issue. Look it up with `gh issue view <number>`.

## What this is

`backlog` is a terminal tool that fuzzy-searches a user's game library
aggregated across Steam, Epic, GOG, and Amazon. It is a Rust rewrite of a
prior Python tool (`own`); some structs note that they match "the Python
version's JSON format exactly" for config/cache compatibility. Currently
macOS / Apple Silicon focused.

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

The binary (`main.rs`) is a thin CLI dispatcher over a library crate
(`lib.rs`). All real logic lives in library modules and is exercised by
integration tests in `tests/`, so prefer adding logic to the library rather
than `main.rs`.

Data flows in two stages, decoupled by an on-disk cache so that searching
never hits the network:

1. **Sync** (`sync.rs`) pulls from sources and writes one JSON cache file per
   platform into `~/.cache/backlog/<platform>.json`.
2. **Load + search** (`loader.rs` + `search.rs`) reads all cache files, merges
   them, and fuzzy-matches. The TUI and one-shot `search` both consume this.

The single shared domain type is `Game { name, platform }` (`lib.rs`).

### Sources (`sources/`)
Each source converts an external format into `Vec<Game>`. Two kinds:
- **`steam.rs`** — live Steam Web API calls via `ureq`. Handles owned-games
  fetch, vanity-URL resolution, and `parse_steam_input` (accepts profile URLs,
  vanity names, or numeric IDs). Needs an API key + Steam ID from config.
- **`heroic.rs`** — reads the local Heroic launcher's `store_cache/*.json`
  files (`legendary_library.json` = Epic, `gog_library.json` = GOG with
  `gog-redist` filtered out, `nile_library.json` = Amazon). No network.

### Cache + config (`cache.rs`, `config.rs`)
- Cache files carry `last_updated` (RFC3339), `source`, and `games`. Caches
  older than **7 days** are flagged stale by the loader (surfaced as
  warnings, not errors). Reads of missing/malformed files return `None` and
  are silently skipped, never panic.
- Config lives at `~/.config/backlog/config.json` (`steam_api_key`,
  `steam_id`).
- Both modules resolve paths from `$HOME`, falling back to `.` when unset —
  this fallback is what makes them testable with `tempfile`.

### Search (`search.rs`)
Uses `nucleo-matcher`. Multi-word queries split into atoms that must all
match. Results sort by score descending, then a relative threshold
(`best * 3/5`) prunes weak matches. `match_indices` are returned for TUI
highlighting.

### TUI (`tui.rs`)
`ratatui` + `crossterm`. `run_with(LoadResult)` is the entry point used by
`main.rs` (it loads the cache first, syncing on first run when no cache
exists). Shows live fuzzy results as the user types, plus a human-readable
"synced Nd ago" age from `loader::format_sync_age`.

## Conventions

- **Errors over panics in library code.** Source/cache/config readers return
  `Option`/`Result` and degrade gracefully (skip a platform, emit a warning)
  rather than aborting. `main.rs` is where user-facing messages and
  `process::exit` live.
- **Status reporting via enums.** Sync results use
  `SyncStatus::{Ok, Skipped, Error}` with a per-platform `SyncReport` rather
  than bubbling errors up — one failing source must not stop the others.
- Tests are integration-style in `tests/`, one file per module, driving the
  public library API with `tempfile` for filesystem isolation. Add coverage
  there when changing library behavior.

## Durable design decisions

- **Named ANSI colors only.** Use the 16 named terminal colors (e.g.
  `Color::Cyan`, `Color::DarkGray`, `Color::White`), never `Color::Rgb`. This
  is what lets the TUI inherit the user's terminal theme rather than
  imposing fixed colors. `tui.rs:398`'s `Color::Rgb(30, 30, 30)`
  zebra-stripe background is a known existing violation, tracked as its own
  follow-up rather than fixed here.

## Working in this repo

- **Always work in a dedicated `wt switch --create jeffdt/<domain>-<brief-desc>`
  worktree, never directly in this main checkout.** Create the worktree
  first, even for an edit that feels too small to bother. A worktree per
  feature branch is what keeps concurrent or resumed sessions safe; the main
  checkout should stay clean and always reflect `origin/main`.
- **When pulling an issue from GitHub to work on, check the other open
  issues for ones that would make sense to bundle into the same PR**
  (`gh issue list --state open`) before starting. Look for genuine overlap,
  e.g. same code area, same setting/UI surface, or one is a natural
  extension of the other, not just a shared label. If a good bundle
  candidate turns up, confirm with the user before folding it in rather than
  assuming; if nothing overlaps, note briefly that none were found and move
  on with just the requested issue.
- **Mock up visual/rendering changes before writing the spec.** When a design
  discussion touches how the TUI renders (colors, layout, new
  glyphs/columns), don't rely on a text description alone — render an ANSI
  mockup (never the real binary) so the user can look at it before design
  gets locked in. See the `mockup` skill for the standardized construction
  method, dimensions, color constraints, and cleanup rules. Skip this for
  changes with no visual surface (model/logic-only work).
- **Leave a live preview when a feature is done.** Once a feature is
  implemented and tests pass, use the built-in `run` skill to launch the
  freshly built binary so the change is confirmed against the real running
  app, not just green test output.
- **Changes land via pull request.** Work on a feature branch named
  `jeffdt/<domain>-<brief-kebab-desc>` (the global convention applies here).
  When the user clears a change to go live, open a PR and then merge it
  yourself (squash, to keep `main` linear) purely for the audit trail; this
  is a solo project with no human review gate, so the PR exists for
  history, not approval. If the session was kicked off from a GitHub issue
  on this repo, reference it in the PR body with `Closes #N` so the issue
  links and auto-closes on merge.
