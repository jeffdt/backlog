---
name: mockup
description: >-
  Use when building a terminal/ANSI mockup for a design discussion, before
  locking in a visual or rendering change (AGENTS.md's "mock up
  visual/rendering changes before writing the spec" step). Triggers include
  "mock this up", "show me a mockup", "render this as ANSI", "let's compare
  a couple of layouts". Do NOT use for the separate live-binary-preview
  workflow (launching the real compiled binary via the `run` skill), since
  that already runs real code and has no quality-consistency problem to fix.
---

# Terminal mockup

Standardizes how fake (not-real-binary) ANSI terminal mockups get built for
design discussions, so they no longer vary in quality by construction
method, window naming, or dimension accuracy.

Unlike rolomux/boomerang, backlog's TUI is not launched into a fixed-width
tmux popup — it fills whatever terminal window it's given. Mockups here use
a representative full-terminal size instead of a popup card.

## 1. Start: task ID and topic

Generate one short ID at the start of the mockup task, reused for every
window/pane spawned during it:

```bash
id=$(date +%H%M%S)
```

Pick a topic: the GitHub issue number if this work is tied to one (e.g.
`70`), else a short kebab-case description (e.g. `zebra-stripe-color`).
Window titles are always `<topic>-mockup-<id>` (never spaces, ever).

## 2. Pick a construction method

**Default: Python.** Use for the common case: text/layout mockups that
don't need to prove an exact color match or a genuinely complex multi-panel
widget layout.

**Escalate to ratatui** only when: the mockup needs to verify an exact color
against the app's real palette, or the layout is complex enough that a
hand-approximated widget risks misleading the design discussion. Ratatui
mockups cost meaningfully more (boilerplate widget/layout code, a
`cargo build`/`run` cycle, real risk of a compile-error round-trip), so
don't reach for it by default.

## 3. Default construction: Python (stdlib only)

Write the mockup script to the Claude Code scratchpad directory (never into
the repo). Every mockup uses this helper; never hand-pad a row:

```python
import re

ANSI_FG = {
    'Black': 30, 'Red': 31, 'Green': 32, 'Yellow': 33,
    'Blue': 34, 'Magenta': 35, 'Cyan': 36, 'Gray': 37,
    'DarkGray': 90, 'LightRed': 91, 'LightGreen': 92, 'LightYellow': 93,
    'LightBlue': 94, 'LightMagenta': 95, 'LightCyan': 96, 'White': 97,
}
ANSI_BG = {name: code + 10 for name, code in ANSI_FG.items()}
RESET = "\x1b[0m"

def fg(name: str) -> str:
    return f"\x1b[{ANSI_FG[name]}m"

def bg(name: str) -> str:
    return f"\x1b[{ANSI_BG[name]}m"

ANSI_RE = re.compile(r'\x1b\[[0-9;]*m')

def visible_width(s: str) -> int:
    return len(ANSI_RE.sub('', s))

def row(content: str, width: int) -> str:
    pad = width - visible_width(content)
    return f"{content}{' ' * max(pad, 0)}"
```

`visible_width` always measures the ANSI-stripped string, so padding lands
correctly regardless of how many color codes are embedded earlier in that
row. `wcwidth` is unnecessary as long as backlog's TUI stays plain ASCII
with no wide characters.

Example full mockup (a search results list, one row highlighted):

```python
WIDTH = 100

def main():
    print(row(f"{fg('White')} search: hollow knight{RESET}", WIDTH))
    print(row("", WIDTH))
    rows = [
        ("Hollow Knight", "steam", True),
        ("Hollow Knight: Silksong", "steam", False),
        ("Knight Squad", "epic", False),
    ]
    for name, platform, selected in rows:
        line = f" {fg('Cyan')}{name}{RESET}  {fg('Blue')}{platform}{RESET}"
        line_bg = bg('DarkGray') if selected else ''
        print(f"{line_bg}{row(line, WIDTH)}{RESET}")

if __name__ == "__main__":
    main()
```

## 4. Escalation: ratatui-rendered mockups

Write a throwaway `examples/mockup.rs` in the project repo, always this
exact filename (one standing `.gitignore` entry, never a new name per
mockup). It builds the screen from real `ratatui` widgets using
`CrosstermBackend<Vec<u8>>`, which captures the actual SGR/cursor bytes
crossterm would emit to a real terminal: the same rendering path the
shipped binary uses, so alignment and color output are correct by
construction, not by care.

Use `Viewport::Fixed` explicitly. Without it, `CrosstermBackend::size()`
queries the real attached terminal's size via `crossterm::terminal::size()`,
which is wrong (or outright errors, since the agent's Bash tool has no
controlling tty) when rendering headlessly. A fixed viewport sidesteps this
entirely.

`CrosstermBackend::writer()` is gated behind an unstable ratatui feature and
isn't callable on the pinned version, so don't rely on it to get the bytes
back out. Instead wrap the `Vec<u8>` in `Rc<RefCell<...>>` and keep your own
handle to it:

```rust
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal, TerminalOptions, Viewport,
};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

#[derive(Clone)]
struct SharedBuf(Rc<RefCell<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() -> std::io::Result<()> {
    let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
    let backend = CrosstermBackend::new(SharedBuf(buf.clone()));
    let viewport = Viewport::Fixed(Rect::new(0, 0, WIDTH, HEIGHT));
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;
    terminal.draw(|frame| {
        let area = Rect::new(0, 0, WIDTH, HEIGHT);
        let row = Paragraph::new(Line::from(Span::styled(
            "Hollow Knight",
            Style::default().fg(Color::Cyan).bg(Color::DarkGray),
        )));
        frame.render_widget(row, Rect::new(area.x, area.y, area.width, 1));
    })?;
    let bytes = buf.borrow();
    print!("{}", String::from_utf8_lossy(&bytes));
    Ok(())
}
```

Run via `cargo run --example mockup --quiet`. backlog is a lib+bin crate, so
the example can `use backlog::...` directly to reuse real functions (e.g.
`platform_color`) rather than hand-copying constants.

Note: crossterm serializes ratatui's named colors as `\x1b[38;5;0` through
`\x1b[38;5;15` (an indexed SGR form), not the classic `\x1b[30`-`\x1b[37`/
`\x1b[90`-`\x1b[97` form the Python path uses. That's still the terminal's
themed 16-color palette, not true 256-color or RGB; indices 0-15 are
exactly the same 16 named colors, just addressed differently. Don't mistake
a `38;5;N`/`48;5;N` (N <= 15) match for a color-fidelity violation when
checking ratatui-path output.

## 5. Standards (both methods)

- **Size**: no fixed popup width here — backlog's TUI fills whatever
  terminal it's launched in. Use a representative size of **100 columns x
  30 rows** unless the discussion specifically concerns behavior at a
  narrower/shorter size, in which case mock up that size instead and say so.
- **Colors**: only the 16 named ANSI colors shown in the `ANSI_FG`/`ANSI_BG`
  tables above (matches `ratatui::style::Color`'s named, non-RGB variants);
  never invent a color the real TUI couldn't produce.

## 6. Launching

```bash
tab=$(mux spawn --workspace caller --title "${topic}-mockup-${id}" --json | jq -r .tab)
```

Always prefix the actual render command with `clear &&` when sending it to
the pane: not needed for the Python path (sequential `print()` never uses
absolute cursor positioning), but required for the ratatui path, since
`CrosstermBackend`'s diffing renderer emits absolute `MoveTo(x,y)` codes
that would otherwise land on top of whatever the pane's shell prompt already
printed:

```bash
mux send --tab "$tab" --cmd "clear && python3 /path/to/scratch_mockup.py"
# or, for the ratatui path:
mux send --tab "$tab" --cmd "clear && cargo run --example mockup --quiet"
```

**Comparing 2+ options**: spawn one window, then add a real tmux pane per
extra option (never a separate window, never a virtualized text trick):

```bash
tmux split-window -h -t "$tab"   # left-right, if the window is wide enough
tmux split-window -t "$tab"      # stacked top-bottom otherwise (tmux default)
```

Write one script per option and label each variant inline in its own title
row (e.g. `backlog -- BEFORE zebra` / `backlog -- AFTER zebra`) so the two
panes are distinguishable at a glance without relying on pane position or
memory of which command went where.

`mux send --tab` only reaches a tab's *active* pane, so once split, target
each pane directly. Grab their IDs first, then send each variant's command
to its own pane:

```bash
tmux list-panes -t "$tab" -F '#{pane_id}'
tmux send-keys -t "%142" "clear && python3 /path/to/variant_a.py" Enter
tmux send-keys -t "%143" "clear && python3 /path/to/variant_b.py" Enter
```

## 7. Cleanup (mandatory, every time)

Once you've reacted to the mockup (approved it, asked for changes, or
moved on), tear down unconditionally, not just on approval:

```bash
mux close --tab "$tab"
rm -f /path/to/scratch_mockup.py
rm -f examples/mockup.rs   # only if the ratatui path was used
```

Deleting `examples/mockup.rs` isn't just tidiness: it sits under `cargo
clippy --all-targets`, part of the normal build/test loop. Leaving it around
would break the *next*, unrelated `cargo clippy` run.
