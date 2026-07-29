# backlog

Fuzzy-search your game library across Steam, Epic, GOG, and Amazon from the
terminal. Type a few letters and it finds the game, no matter which launcher
owns it, then queue up what you mean to play next.

![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust&logoColor=white)
![TUI](https://img.shields.io/badge/TUI-ratatui-1f6feb)
![License](https://img.shields.io/badge/license-MIT-green)
![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20Silicon)-lightgrey)
![Vibe coded](https://img.shields.io/badge/vibe%20coded-100%25-ff69b4)

## Setup

```sh
cargo install --path .
backlog setup
```

The `setup` command prompts for your
[Steam API key](https://steamcommunity.com/dev/apikey) and your Steam profile
URL or vanity name (e.g. `https://steamcommunity.com/id/yourname` or just
`yourname`). It resolves the vanity name to a Steam ID automatically and stores
credentials in `~/.config/backlog/config.json`.

## Usage

```sh
# One-shot search
backlog "hollow knight"

# Sync library caches from all sources
backlog sync

# Print your ranked queue
backlog queue

# Interactive TUI
backlog

# Print the installed version
backlog --version
```

## Sources

- **Steam** via the Web API (requires API key)
- **Epic / GOG / Amazon** via Heroic launcher's local library files

## Queue

Mark games as queued (want to play soon) or played from the TUI, then
filter and reorder the list. Queue state lives in
`~/.local/share/backlog/queue.json`. If that file can't be read or parsed,
backlog says so and stops saving rather than overwriting it, so a bad
hand-edit never costs you the queue.

| Key | Action |
| --- | --- |
| `Enter` | Toggle queued on the selected game |
| `Ctrl+P` | Toggle played on the selected game |
| `Tab` / `Shift+Tab` | Cycle filter: all, queued, played, unplayed |
| `Ctrl+J` / `Ctrl+K` (or `Shift+↓` / `Shift+↑`) | Move the selected game down / up in rank (queued filter only) |
| `Ctrl+R` | Force-sync libraries |
| `Esc` | Quit |

## Development

```sh
cargo build
cargo test
cargo run
```

This repo also ships two Claude Code skills for working on it visually:
`mockup`, for comparing ANSI mockups of a design change before
implementing it, and `live-preview`, for popping the freshly built binary
open in a real tmux window once a feature is done. Both come from the
`tui-utils` plugin:

```
/plugin marketplace add jeffdt/tui-utils
/plugin install tui-utils@tui-utils
```
