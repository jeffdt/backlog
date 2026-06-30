# backlog

Fuzzy-search your game library across Steam, Epic, GOG, and Amazon from the
terminal. Type a few letters and it finds the game, no matter which launcher
owns it.

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
[Steam API key](https://steamcommunity.com/dev/apikey) and Steam ID, then
stores them in `~/.config/backlog/config.json`.

## Usage

```sh
# One-shot search
backlog search "hollow knight"

# Sync library caches from all sources
backlog sync

# Interactive TUI
backlog
```

## Sources

- **Steam** via the Web API (requires API key)
- **Epic / GOG / Amazon** via Heroic launcher's local library files
