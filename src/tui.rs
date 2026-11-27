use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::LibraryEntry;
use crate::cache;
use crate::loader::{self, LoadResult};
use crate::queue::{Filter, PLAYED_GLYPH, QUEUED_GLYPH, apply_filter};
use crate::search;
use crate::sync;
use crate::sync::{SyncReport, SyncStatus};

/// Platform cache filenames written by `sync::sync_all`, used to snapshot
/// per-platform counts before a sync so the status row can show a delta.
const SYNCED_PLATFORMS: [&str; 4] = ["epic", "gog", "amazon", "steam"];

enum SyncState {
    Idle,
    Syncing,
    Done(Vec<SyncReport>),
}

struct App {
    input: String,
    cursor_pos: usize,
    games: Vec<LibraryEntry>,
    selected: usize,
    sync_age: String,
    quit: bool,
    sync_state: SyncState,
    sync_rx: Option<mpsc::Receiver<Vec<SyncReport>>>,
    sync_before_counts: HashMap<String, usize>,
    cache_dir: PathBuf,
    heroic_dir: PathBuf,
    config_path: PathBuf,
    queue: crate::queue::Queue,
    queue_path: PathBuf,
    /// False when the queue file failed to load, so a file we could not read
    /// is never replaced by the empty queue standing in for it.
    queue_writable: bool,
    filter: crate::queue::Filter,
    /// Queue load or write failure, shown in place of the hint until the next
    /// successful write.
    status_message: Option<String>,
}

impl App {
    fn new(
        load_result: LoadResult,
        cache_dir: PathBuf,
        heroic_dir: PathBuf,
        config_path: PathBuf,
        queue_path: PathBuf,
    ) -> Self {
        let sync_age = loader::format_sync_age(load_result.oldest_update);
        let (queue, queue_writable, status_message) = match crate::queue::load(&queue_path) {
            Ok(queue) => (queue, true, None),
            Err(e) => (crate::queue::Queue::default(), false, Some(e)),
        };
        Self {
            input: String::new(),
            cursor_pos: 0,
            games: load_result.games,
            selected: 0,
            sync_age,
            quit: false,
            sync_state: SyncState::Idle,
            sync_rx: None,
            sync_before_counts: HashMap::new(),
            cache_dir,
            heroic_dir,
            config_path,
            queue,
            queue_path,
            queue_writable,
            filter: crate::queue::Filter::default(),
            status_message,
        }
    }

    /// The library narrowed to the active filter, in the order it renders.
    fn filtered(&self) -> Vec<LibraryEntry> {
        crate::queue::apply_filter(&self.games, &self.queue, self.filter)
    }

    fn start_sync(&mut self) {
        if matches!(self.sync_state, SyncState::Syncing) {
            return;
        }
        self.sync_before_counts = SYNCED_PLATFORMS
            .iter()
            .map(|platform| {
                let count = cache::read_cache(&self.cache_dir.join(format!("{platform}.json")))
                    .map(|c| c.games.len())
                    .unwrap_or(0);
                (platform.to_string(), count)
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        let cache_dir = self.cache_dir.clone();
        let heroic_dir = self.heroic_dir.clone();
        let config_path = self.config_path.clone();
        thread::spawn(move || {
            let reports = sync::sync_all(&heroic_dir, &config_path, &cache_dir);
            let _ = tx.send(reports);
        });
        self.sync_rx = Some(rx);
        self.sync_state = SyncState::Syncing;
    }

    fn poll_sync(&mut self) {
        let Some(rx) = &self.sync_rx else {
            return;
        };
        let Ok(reports) = rx.try_recv() else {
            return;
        };
        let load_result = loader::load_all_games(&self.cache_dir);
        self.games = load_result.games;
        self.sync_age = loader::format_sync_age(load_result.oldest_update);
        self.selected = 0;
        self.sync_state = SyncState::Done(reports);
        self.sync_rx = None;
    }

    /// The cursor position clamped to the currently visible rows, paired with
    /// the game at that row. `None` when there are no visible rows.
    fn selected_row(&self) -> Option<(usize, String)> {
        let filtered = self.filtered();
        let results = search::fuzzy_search(&self.input, &filtered);
        if results.is_empty() {
            return None;
        }
        let clamped = self.selected.min(results.len() - 1);
        results.get(clamped).map(|r| (clamped, r.game.name.clone()))
    }

    /// The game under the cursor, resolved against the currently visible rows.
    ///
    /// Mirrors `draw`'s guard: the blank default screen (empty query, `All`
    /// filter) shows no rows, so it must not resolve to a game either.
    fn selected_game(&self) -> Option<String> {
        if self.input.is_empty() && self.filter == crate::queue::Filter::All {
            return None;
        }
        self.selected_row().map(|(_, name)| name)
    }

    /// Persists the queue, surfacing a write failure in the status row rather
    /// than tearing down the terminal.
    ///
    /// Refuses to write at all when the file failed to load: the in-memory
    /// queue is then an empty stand-in, not the user's state.
    fn persist_queue(&mut self) {
        if !self.queue_writable {
            return;
        }
        self.status_message = match crate::queue::save(&self.queue_path, &self.queue) {
            Ok(()) => None,
            Err(e) => Some(format!(
                "queue not saved: {e} ({})",
                self.queue_path.display()
            )),
        };
    }

    fn toggle_queued(&mut self) {
        let Some(name) = self.selected_game() else {
            return;
        };
        self.queue.toggle_queued(&name);
        self.persist_queue();
    }

    fn toggle_played(&mut self) {
        let Some(name) = self.selected_game() else {
            return;
        };
        self.queue.toggle_played(&name);
        self.persist_queue();
    }

    /// Moves the selected game one rank, keeping the cursor on it.
    ///
    /// Only meaningful under the queued filter. The cursor is re-derived from
    /// the moved game's position in the freshly filtered rows rather than
    /// nudged by one: a swap across a queued game missing from the library
    /// changes rank without moving any visible row, and with a search active
    /// rows are score-ordered rather than rank-ordered.
    fn move_selected(&mut self, down: bool) {
        if self.filter != crate::queue::Filter::Queued {
            return;
        }
        let Some((clamped, name)) = self.selected_row() else {
            return;
        };
        self.selected = clamped;
        let moved = if down {
            self.queue.move_down(&name)
        } else {
            self.queue.move_up(&name)
        };
        if !moved {
            return;
        }
        let filtered = self.filtered();
        if let Some(row) = search::fuzzy_search(&self.input, &filtered)
            .iter()
            .position(|r| r.game.name == name)
        {
            self.selected = row;
        }
        self.persist_queue();
    }
}

/// Launches the interactive TUI, loading games from `cache_dir`.
pub fn run(cache_dir: &Path) -> io::Result<()> {
    let load_result = loader::load_all_games(cache_dir);
    let heroic_dir = crate::sources::heroic::heroic_store_cache_dir();
    let config_path = crate::config::default_config_path();
    let queue_path = crate::queue::default_queue_path();
    run_with(
        load_result,
        cache_dir.to_path_buf(),
        heroic_dir,
        config_path,
        queue_path,
    )
}

/// Launches the interactive TUI with a pre-loaded game list.
pub fn run_with(
    load_result: LoadResult,
    cache_dir: PathBuf,
    heroic_dir: PathBuf,
    config_path: PathBuf,
    queue_path: PathBuf,
) -> io::Result<()> {
    for w in &load_result.warnings {
        eprintln!("Warning: {w}");
    }

    if load_result.games.is_empty() {
        eprintln!("No games loaded. Run `bouncer setup` to configure, then `bouncer sync`.");
        return Ok(());
    }

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(load_result, cache_dir, heroic_dir, config_path, queue_path);
    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

const TICK_RATE: Duration = Duration::from_millis(100);

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        app.poll_sync();
        terminal.draw(|f| draw(f, app))?;

        if app.quit {
            return Ok(());
        }

        if event::poll(TICK_RATE)?
            && let Event::Key(key) = event::read()?
        {
            match (key.code, key.modifiers) {
                (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    app.quit = true;
                }
                (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                    app.start_sync();
                }
                (KeyCode::BackTab, _) => {
                    app.filter = app.filter.prev();
                    app.selected = 0;
                }
                (KeyCode::Tab, _) => {
                    app.filter = app.filter.next();
                    app.selected = 0;
                }
                (KeyCode::Enter, _) => {
                    app.toggle_queued();
                }
                (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                    app.toggle_played();
                }
                (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                    app.move_selected(true);
                }
                (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                    app.move_selected(false);
                }
                (KeyCode::Down, KeyModifiers::SHIFT) => {
                    app.move_selected(true);
                }
                (KeyCode::Up, KeyModifiers::SHIFT) => {
                    app.move_selected(false);
                }
                (KeyCode::Char(c), _) => {
                    app.input.insert(app.cursor_pos, c);
                    app.cursor_pos += c.len_utf8();
                    app.selected = 0;
                }
                (KeyCode::Backspace, _) if app.cursor_pos > 0 => {
                    let prev = app.input[..app.cursor_pos]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    app.input.drain(prev..app.cursor_pos);
                    app.cursor_pos = prev;
                    app.selected = 0;
                }
                (KeyCode::Left, _) if app.cursor_pos > 0 => {
                    app.cursor_pos = app.input[..app.cursor_pos]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                (KeyCode::Right, _) if app.cursor_pos < app.input.len() => {
                    app.cursor_pos = app.input[app.cursor_pos..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| app.cursor_pos + i)
                        .unwrap_or(app.input.len());
                }
                (KeyCode::Down, _) => {
                    app.selected = app.selected.saturating_add(1);
                }
                (KeyCode::Up, _) => {
                    app.selected = app.selected.saturating_sub(1);
                }
                _ => {}
            }
        }
    }
}

/// Renders the queue marker field as colored spans.
fn marker_spans(state: crate::queue::GameState) -> Vec<Span<'static>> {
    let queued = if state.queued {
        Span::styled(QUEUED_GLYPH.to_string(), Style::default().fg(Color::Cyan))
    } else {
        Span::raw(" ")
    };
    let played = if state.played {
        Span::styled(PLAYED_GLYPH.to_string(), Style::default().fg(Color::Green))
    } else {
        Span::raw(" ")
    };
    vec![queued, played, Span::raw(" ")]
}

const FILTERS: [Filter; 4] = [
    Filter::All,
    Filter::Queued,
    Filter::Played,
    Filter::Unplayed,
];

/// Bouncer's own color: the frame border and its corner titles.
const ACCENT: Color = Color::Magenta;

/// The color that marks a filter wherever it is active: its name in the
/// filter row and the section header and gutter below it.
fn filter_color(filter: Filter) -> Color {
    match filter {
        Filter::All => Color::Magenta,
        Filter::Queued => Color::Cyan,
        Filter::Played => Color::Green,
        Filter::Unplayed => Color::Yellow,
    }
}

/// One line naming every filter with its row count; the active one is bold
/// in its filter color, the rest dim. `counts` is indexed in `FILTERS` order.
fn filter_row(active: Filter, counts: [usize; 4]) -> Line<'static> {
    let dim = Style::default().fg(Color::DarkGray);
    let mut spans = Vec::new();
    for (i, (filter, count)) in FILTERS.into_iter().zip(counts).enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ·   ", dim));
        }
        if let Some(glyph) = filter.glyph() {
            spans.push(Span::styled(
                format!("{glyph} "),
                Style::default().fg(filter_color(filter)),
            ));
        }
        let is_active = filter == active;
        let label_style = if is_active {
            Style::default()
                .fg(filter_color(filter))
                .add_modifier(Modifier::BOLD)
        } else {
            dim
        };
        spans.push(Span::styled(filter.label(), label_style));
        let count_color = if is_active {
            Color::Gray
        } else {
            Color::DarkGray
        };
        spans.push(Span::styled(
            format!(" {count}"),
            Style::default().fg(count_color),
        ));
    }
    Line::from(spans)
}

/// A bold section title followed by a rule in `color` running to `width`,
/// with an optional dim `tail` right-aligned at the end.
fn section_header(title: &str, color: Color, tail: Option<&str>, width: u16) -> Line<'static> {
    let width = width as usize;
    let title_len = title.chars().count();
    let tail = tail
        .map(|t| format!(" {t}"))
        .filter(|t| title_len + 2 + t.chars().count() <= width)
        .unwrap_or_default();
    let gap = if width > title_len { " " } else { "" };
    let rule = "─".repeat(width.saturating_sub(title_len + gap.len() + tail.chars().count()));
    Line::from(vec![
        Span::styled(
            title.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(gap),
        Span::styled(rule, Style::default().fg(color)),
        Span::styled(tail, Style::default().fg(Color::DarkGray)),
    ])
}

/// Styles a `"key desc · key desc"` hint so each key stands a step brighter
/// than its description, matching the footer of rolomux and boomerang.
fn styled_hint(text: &str) -> Line<'static> {
    let dim = Style::default().fg(Color::DarkGray);
    let mut spans = Vec::new();
    for (i, segment) in text.split(" · ").enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", dim));
        }
        match segment.split_once(' ') {
            Some((key, desc)) => {
                spans.push(Span::styled(
                    key.to_string(),
                    Style::default().fg(Color::Gray),
                ));
                spans.push(Span::styled(format!(" {desc}"), dim));
            }
            None => spans.push(Span::styled(segment.to_string(), dim)),
        }
    }
    Line::from(spans)
}

/// The footer's key hint, naming only keys that do something right now:
/// row keys only when rows exist, ranking only under the queued filter, and
/// enter as "unqueue" when the selected game is already queued.
fn hint_text(filter: Filter, has_rows: bool, selected_queued: bool) -> &'static str {
    match (has_rows, filter == Filter::Queued, selected_queued) {
        (false, _, _) => "type search · tab filter · ^r sync · esc quit",
        (true, true, _) => {
            "↑↓ move · enter unqueue · ^p played · ^j/^k rank · tab filter · esc quit"
        }
        (true, false, true) => "↑↓ move · enter unqueue · ^p played · tab filter · esc quit",
        (true, false, false) => "↑↓ move · enter queue · ^p played · tab filter · esc quit",
    }
}

/// Row counts for each filter tab, in `FILTERS` order.
fn view_counts(app: &App) -> [usize; 4] {
    FILTERS.map(|f| apply_filter(&app.games, &app.queue, f).len())
}

fn platform_color(platform: &str) -> Color {
    match platform {
        "steam" => Color::Blue,
        "epic" => Color::White,
        "gog" => Color::Magenta,
        "amazon" => Color::Yellow,
        _ => Color::Gray,
    }
}

fn format_sync_summary(reports: &[SyncReport], before_counts: &HashMap<String, usize>) -> String {
    reports
        .iter()
        .map(|r| {
            let value = match &r.status {
                SyncStatus::Ok => {
                    let before = before_counts.get(&r.platform).copied().unwrap_or(0);
                    let delta = r.game_count as i64 - before as i64;
                    if delta >= 0 {
                        format!("+{delta}")
                    } else {
                        format!("{delta}")
                    }
                }
                SyncStatus::Skipped(_) => "skip".to_string(),
                SyncStatus::Error(_) => "error".to_string(),
            };
            format!("{} {}", r.platform, value)
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// One game row exactly `width` columns wide: the section gutter in
/// `gutter`, an optional rank, queue markers, the name with match
/// highlighting (cut short with `…` when it would crowd the platforms), then
/// the platform list flush against the right edge. Row background is left to
/// the caller.
fn result_row_spans(
    app: &App,
    name: &str,
    platforms: &[String],
    match_indices: &[u32],
    width: usize,
    rank: Option<usize>,
    gutter: Color,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled("│", Style::default().fg(gutter))];
    if let Some(rank) = rank {
        spans.push(Span::styled(
            format!("{rank:<2} "),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let state = app.queue.state(name);
    spans.extend(marker_spans(state));
    let prefix_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();

    let mut platform_spans = Vec::new();
    for (i, platform) in platforms.iter().enumerate() {
        if i > 0 {
            platform_spans.push(Span::styled(" / ", Style::default().fg(Color::Gray)));
        }
        platform_spans.push(Span::styled(
            platform.clone(),
            Style::default()
                .fg(platform_color(platform))
                .add_modifier(Modifier::ITALIC),
        ));
    }
    let platforms_len: usize = platform_spans
        .iter()
        .map(|s| s.content.chars().count())
        .sum();

    let base_color = if state.played {
        Color::Gray
    } else {
        Color::White
    };
    let name_len = name.chars().count();
    let name_budget = width.saturating_sub(prefix_len + platforms_len + 1);
    let (shown, ellipsis) = if name_len <= name_budget {
        (name_len, false)
    } else {
        (name_budget.saturating_sub(1), name_budget > 0)
    };
    spans.extend(name.chars().take(shown).enumerate().map(|(char_idx, ch)| {
        let style = if match_indices.contains(&(char_idx as u32)) {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base_color)
        };
        Span::styled(ch.to_string(), style)
    }));
    if ellipsis {
        spans.push(Span::styled("…", Style::default().fg(base_color)));
    }

    let used = prefix_len + shown + usize::from(ellipsis) + platforms_len;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    spans.extend(platform_spans);
    clip_spans(spans, width)
}

/// Drops whatever runs past `width` columns, so a row never spills over on a
/// terminal too narrow for its markers and platforms.
fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut left = width;
    let mut out = Vec::new();
    for span in spans {
        if left == 0 {
            break;
        }
        let len = span.content.chars().count();
        if len <= left {
            left -= len;
            out.push(span);
        } else {
            let kept: String = span.content.chars().take(left).collect();
            out.push(Span::styled(kept, span.style));
            left = 0;
        }
    }
    out
}

const UP_NEXT_ROWS: usize = 3;

/// Body of the blank default screen, header first: the top of the queue for
/// returning users, or a short key guide when the queue is empty.
fn empty_state_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let queued = apply_filter(&app.games, &app.queue, Filter::Queued);
    let dim = Style::default().fg(Color::DarkGray);

    if queued.is_empty() {
        let color = Color::Blue;
        let key = |k: &str, desc: Vec<Span<'static>>| {
            let mut spans = vec![
                Span::styled("│", Style::default().fg(color)),
                Span::styled(format!(" {k:<6}"), Style::default().fg(Color::Gray)),
            ];
            spans.extend(desc);
            Line::from(spans)
        };
        let text = |t: &str| Span::styled(t.to_string(), dim);
        return vec![
            section_header("GETTING STARTED", color, None, width),
            key(
                "type",
                vec![text("fuzzy-search every game you own, across all stores")],
            ),
            key(
                "enter",
                vec![
                    text("add the selected game to your queue "),
                    Span::styled(QUEUED_GLYPH.to_string(), Style::default().fg(Color::Cyan)),
                ],
            ),
            key(
                "^p",
                vec![
                    text("mark it played "),
                    Span::styled(PLAYED_GLYPH.to_string(), Style::default().fg(Color::Green)),
                ],
            ),
            key(
                "tab",
                vec![text("switch between all / queued / played / unplayed")],
            ),
        ];
    }

    let color = filter_color(Filter::All);
    let tail = format!("{} queued", queued.len());
    let shown = &queued[..queued.len().min(UP_NEXT_ROWS)];
    let mut lines = vec![section_header("UP NEXT", color, Some(&tail), width)];
    lines.extend(shown.iter().enumerate().map(|(i, e)| {
        Line::from(result_row_spans(
            app,
            &e.name,
            &e.platforms,
            &[],
            width as usize,
            Some(i + 1),
            color,
        ))
    }));
    if queued.len() > shown.len() {
        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(color)),
            Span::styled(
                format!(
                    "  +{} more \u{b7} tab to see the full queue",
                    queued.len() - shown.len()
                ),
                dim,
            ),
        ]));
    }
    lines
}

/// A `‹ text ›` corner title in the accent color, with the one-dash gap
/// from the corner that rolomux and boomerang use.
fn corner_title(text: &str, bold: bool, dash_first: bool) -> Line<'static> {
    let border = Style::default().fg(ACCENT);
    let mut emphasis = border.add_modifier(Modifier::ITALIC);
    if bold {
        emphasis = emphasis.add_modifier(Modifier::BOLD);
    }
    let label = Span::styled(format!("‹ {text} ›"), emphasis);
    let dash = Span::styled("─", border);
    if dash_first {
        Line::from(vec![dash, label])
    } else {
        Line::from(vec![label, dash]).right_aligned()
    }
}

/// The section header title for a filter browsed with an empty search.
fn browse_title(filter: Filter) -> &'static str {
    match filter {
        Filter::All => "LIBRARY",
        Filter::Queued => "QUEUE",
        Filter::Played => "PLAYED",
        Filter::Unplayed => "UNPLAYED",
    }
}

fn draw(f: &mut Frame, app: &App) {
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(corner_title("bouncer", true, true))
        .title(corner_title(&app.sync_age, false, false))
        .title_bottom(corner_title(
            &format!("v{}", env!("CARGO_PKG_VERSION")),
            false,
            false,
        ))
        .padding(Padding::new(1, 1, 0, 0));

    let inner = outer_block.inner(f.area());
    f.render_widget(outer_block, f.area());

    let [
        filter_area,
        _,
        search_area,
        _,
        header_area,
        body_area,
        status_area,
        rule_area,
        hint_area,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let dim = Style::default().fg(Color::DarkGray);

    f.render_widget(
        Paragraph::new(filter_row(app.filter, view_counts(app))),
        filter_area,
    );

    const SEARCH_PROMPT: &str = "search: ";
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(SEARCH_PROMPT, dim),
            Span::raw(app.input.clone()),
        ])),
        search_area,
    );
    let cursor_x = search_area.x
        + SEARCH_PROMPT.len() as u16
        + app.input[..app.cursor_pos].chars().count() as u16;
    f.set_cursor_position((cursor_x, search_area.y));

    let show_empty_state = app.input.is_empty() && app.filter == Filter::All;
    let filtered = app.filtered();
    let results = if show_empty_state {
        Vec::new()
    } else {
        search::fuzzy_search(&app.input, &filtered)
    };
    let selected = app.selected.min(results.len().saturating_sub(1));
    let selected_queued = results
        .get(selected)
        .is_some_and(|r| app.queue.state(&r.game.name).queued);

    // A queue problem outranks the sync summary: it is the only state here the
    // user cannot regenerate by syncing again.
    let status = match (&app.status_message, &app.sync_state) {
        (Some(message), _) => Span::styled(message.clone(), Style::default().fg(Color::Red)),
        (None, SyncState::Syncing) => Span::styled("syncing...", dim),
        (None, SyncState::Done(reports)) => {
            Span::styled(format_sync_summary(reports, &app.sync_before_counts), dim)
        }
        (None, SyncState::Idle) => Span::raw(""),
    };
    f.render_widget(Paragraph::new(Line::from(status)), status_area);
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(rule_area.width as usize), dim)),
        rule_area,
    );
    f.render_widget(
        Paragraph::new(styled_hint(hint_text(
            app.filter,
            !results.is_empty(),
            selected_queued,
        ))),
        hint_area,
    );

    if show_empty_state {
        let mut lines = empty_state_lines(app, body_area.width);
        let header = lines.remove(0);
        f.render_widget(Paragraph::new(header), header_area);
        f.render_widget(Paragraph::new(lines), body_area);
        return;
    }

    let color = filter_color(app.filter);
    let (title, tail) = if app.input.is_empty() {
        (
            browse_title(app.filter),
            format!("{} games", filtered.len()),
        )
    } else {
        (
            "RESULTS",
            format!("{} of {}", results.len(), filtered.len()),
        )
    };
    f.render_widget(
        Paragraph::new(section_header(title, color, Some(&tail), header_area.width)),
        header_area,
    );

    let visible_rows = body_area.height as usize;
    let scroll_offset = (selected + 1).saturating_sub(visible_rows);
    for (i, result) in results
        .iter()
        .skip(scroll_offset)
        .take(visible_rows)
        .enumerate()
    {
        let rank = if app.filter == Filter::Queued {
            app.queue.rank(&result.game.name)
        } else {
            None
        };
        let mut spans = result_row_spans(
            app,
            &result.game.name,
            &result.game.platforms,
            &result.match_indices,
            body_area.width as usize,
            rank,
            color,
        );
        if i + scroll_offset == selected {
            for span in spans.iter_mut().skip(1) {
                span.style = span.style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
            }
        }
        let row_area = Rect::new(body_area.x, body_area.y + i as u16, body_area.width, 1);
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{SyncReport, SyncStatus};
    use tempfile::TempDir;

    /// Builds an `App` over an in-memory library, with the queue file rooted
    /// in a scratch directory so `persist_queue` never touches real state.
    fn test_app(queue_dir: &TempDir, games: &[&str]) -> App {
        let load_result = LoadResult {
            games: games
                .iter()
                .map(|name| LibraryEntry {
                    name: name.to_string(),
                    platforms: vec!["steam".to_string()],
                })
                .collect(),
            warnings: Vec::new(),
            oldest_update: None,
        };
        App::new(
            load_result,
            queue_dir.path().to_path_buf(),
            queue_dir.path().to_path_buf(),
            queue_dir.path().join("config.json"),
            queue_dir.path().join("queue.json"),
        )
    }

    fn ok(platform: &str, count: usize) -> SyncReport {
        SyncReport {
            platform: platform.to_string(),
            game_count: count,
            status: SyncStatus::Ok,
        }
    }

    #[test]
    fn ok_reports_show_full_count_as_delta_when_no_prior_snapshot() {
        let reports = vec![ok("steam", 42), ok("epic", 10)];
        let before = HashMap::new();
        assert_eq!(
            format_sync_summary(&reports, &before),
            "steam +42  epic +10"
        );
    }

    #[test]
    fn ok_reports_show_delta_from_before_counts() {
        let reports = vec![ok("steam", 42), ok("epic", 10)];
        let before = HashMap::from([("steam".to_string(), 39), ("epic".to_string(), 10)]);
        assert_eq!(format_sync_summary(&reports, &before), "steam +3  epic +0");
    }

    #[test]
    fn ok_report_shows_negative_delta_when_games_removed() {
        let reports = vec![ok("steam", 40)];
        let before = HashMap::from([("steam".to_string(), 42)]);
        assert_eq!(format_sync_summary(&reports, &before), "steam -2");
    }

    #[test]
    fn mixed_statuses_render_skip_and_error_words() {
        let reports = vec![
            ok("steam", 42),
            SyncReport {
                platform: "gog".to_string(),
                game_count: 0,
                status: SyncStatus::Skipped("gog_library.json not found".to_string()),
            },
            SyncReport {
                platform: "amazon".to_string(),
                game_count: 0,
                status: SyncStatus::Error("network error".to_string()),
            },
        ];
        let before = HashMap::new();
        assert_eq!(
            format_sync_summary(&reports, &before),
            "steam +42  gog skip  amazon error"
        );
    }

    #[test]
    fn empty_reports_produce_empty_string() {
        assert_eq!(format_sync_summary(&[], &HashMap::new()), "");
    }

    fn marker_chars(state: crate::queue::GameState) -> String {
        marker_spans(state)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn marker_spans_render_glyphs_blanks_and_a_trailing_space() {
        use crate::queue::GameState;
        assert_eq!(
            marker_chars(GameState {
                queued: true,
                played: false
            }),
            "»  "
        );
        assert_eq!(
            marker_chars(GameState {
                queued: false,
                played: true
            }),
            " ✓ "
        );
        assert_eq!(
            marker_chars(GameState {
                queued: true,
                played: true
            }),
            "»✓ "
        );
        assert_eq!(
            marker_chars(GameState {
                queued: false,
                played: false
            }),
            "   "
        );
    }

    #[test]
    fn marker_spans_color_queued_cyan_and_played_green() {
        use crate::queue::GameState;
        let spans = marker_spans(GameState {
            queued: true,
            played: true,
        });
        assert_eq!(spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(spans[1].style.fg, Some(Color::Green));
    }

    #[test]
    fn selected_game_is_none_on_the_blank_default_screen() {
        let dir = TempDir::new().unwrap();
        let app = test_app(&dir, &["Hollow Knight", "Hades"]);
        assert_eq!(app.selected_game(), None);
    }

    #[test]
    fn enter_on_the_blank_default_screen_does_not_queue_anything() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["Hollow Knight", "Hades"]);
        app.toggle_queued();
        assert!(app.queue.entries.is_empty());
    }

    #[test]
    fn selected_game_resolves_once_input_or_filter_shows_rows() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["Hollow Knight", "Hades"]);
        app.input = "hades".to_string();
        assert_eq!(app.selected_game(), Some("Hades".to_string()));
    }

    #[test]
    fn move_selected_follows_the_moved_game_even_when_the_cursor_overshot() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C"]);
        app.queue.toggle_queued("A");
        app.queue.toggle_queued("B");
        app.queue.toggle_queued("C");
        app.filter = crate::queue::Filter::Queued;
        // Plain Down has no upper bound, so holding it past the end leaves
        // `selected` far past the last row.
        app.selected = 100;

        app.move_selected(false);

        assert_eq!(app.queue.rank("C"), Some(2));
        assert_eq!(app.selected, 1, "cursor should self-heal onto C's new row");
    }

    #[test]
    fn move_selected_stays_on_the_game_when_the_swap_moves_no_visible_row() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["Tunic", "Hades"]);
        app.queue.toggle_queued("Tunic");
        app.queue.toggle_queued("Uninstalled Game");
        app.queue.toggle_queued("Hades");
        app.filter = crate::queue::Filter::Queued;
        app.selected = 0;

        app.move_selected(true);

        // Tunic swapped past a queued game absent from the library, so its rank
        // changed but the visible rows did not.
        assert_eq!(app.queue.rank("Tunic"), Some(2));
        assert_eq!(app.selected, 0, "cursor should stay on Tunic");
    }

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn queue_all(app: &mut App, names: &[&str]) {
        for n in names {
            app.queue.toggle_queued(n);
        }
    }

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    #[test]
    fn result_rows_put_gutter_first_and_platforms_at_the_right_edge() {
        let dir = TempDir::new().unwrap();
        let long = "Eye of the Beholder III: Assault on Myth Drannor";
        let mut app = test_app(&dir, &["Hollow Knight", long]);
        queue_all(&mut app, &["Hollow Knight"]);
        type RowCase<'a> = (&'a str, &'a [&'a str], usize, Option<usize>, &'a str);
        let cases: &[RowCase] = &[
            (
                "Hollow Knight",
                &["steam"],
                30,
                None,
                "│»  Hollow Knight        steam",
            ),
            (
                "Hollow Knight",
                &["epic", "gog"],
                32,
                Some(1),
                "│1  »  Hollow Knight  epic / gog",
            ),
            (
                "Hollow Knight",
                &["steam"],
                32,
                Some(12),
                "│12 »  Hollow Knight       steam",
            ),
            (long, &["gog"], 30, None, "│   Eye of the Beholder I… gog"),
        ];
        let mut misses = Vec::new();
        for &(name, platforms, width, rank, want) in cases {
            let platforms: Vec<String> = platforms.iter().map(|p| p.to_string()).collect();
            let got: String =
                result_row_spans(&app, name, &platforms, &[], width, rank, Color::Magenta)
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
            if got != want || got.chars().count() != width {
                misses.push(format!("{got:?} != {want:?}"));
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");

        let spans = result_row_spans(
            &app,
            "Hollow Knight",
            &["steam".into()],
            &[],
            30,
            None,
            Color::Cyan,
        );
        assert_eq!(spans[0].content, "│");
        assert_eq!(spans[0].style.fg, Some(Color::Cyan));
        let steam = spans.iter().find(|s| s.content == "steam").unwrap();
        assert!(steam.style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(steam.style.fg, Some(Color::Blue));
    }

    #[test]
    fn result_rows_survive_widths_too_small_for_the_platform() {
        let dir = TempDir::new().unwrap();
        let app = test_app(&dir, &["Hollow Knight"]);
        for width in 0..12 {
            let row = result_row_spans(
                &app,
                "Hollow Knight",
                &["steam".into()],
                &[0],
                width,
                Some(3),
                Color::Magenta,
            );
            let len: usize = row.iter().map(|s| s.content.chars().count()).sum();
            assert!(len <= width.max(1), "width {width}: {len}");
        }
    }

    #[test]
    fn empty_state_shows_up_next_with_top_three_and_more_count() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D", "E"]);
        queue_all(&mut app, &["A", "B", "C", "D", "E"]);
        let lines = empty_state_lines(&app, 60);
        let t = texts(&lines);
        assert_eq!(t.len(), 5);
        assert!(t[0].starts_with("UP NEXT ─"), "{}", t[0]);
        assert!(t[0].ends_with("─ 5 queued"), "{}", t[0]);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Magenta));
        for (i, name) in ["A", "B", "C"].iter().enumerate() {
            assert!(
                t[i + 1].starts_with(&format!("│{}  ", i + 1)),
                "{}",
                t[i + 1]
            );
            assert!(t[i + 1].contains(name));
        }
        assert_eq!(
            t[4].trim_end(),
            "│  +2 more \u{b7} tab to see the full queue"
        );
        assert_eq!(lines[4].spans[0].style.fg, Some(Color::Magenta));
    }

    #[test]
    fn empty_state_omits_more_line_for_three_or_fewer() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D"]);
        queue_all(&mut app, &["A", "B", "C"]);
        assert_eq!(empty_state_lines(&app, 60).len(), 4);
    }

    #[test]
    fn empty_state_skips_queued_games_missing_from_library() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D"]);
        queue_all(&mut app, &["Gone", "A", "B", "C", "D"]);
        let t = texts(&empty_state_lines(&app, 60));
        assert!(!t.iter().any(|l| l.contains("Gone")));
        assert_eq!(t.len(), 5);
        assert!(t[4].starts_with("│  +1 more"), "{}", t[4]);
    }

    #[test]
    fn empty_state_shows_getting_started_when_queue_is_empty() {
        let dir = TempDir::new().unwrap();
        let app = test_app(&dir, &["A"]);
        let lines = empty_state_lines(&app, 60);
        let t = texts(&lines);
        let expected = [
            ("type", "fuzzy-search every game you own, across all stores"),
            ("enter", "add the selected game to your queue \u{bb}"),
            ("^p", "mark it played \u{2713}"),
            ("tab", "switch between all / queued / played / unplayed"),
        ];
        assert!(t[0].starts_with("GETTING STARTED ─"), "{}", t[0]);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Blue));
        assert_eq!(t.len(), 5);
        for (i, (key, desc)) in expected.iter().enumerate() {
            assert!(t[i + 1].starts_with(&format!("│ {key:<6}")), "{}", t[i + 1]);
            assert!(t[i + 1].ends_with(desc), "{}", t[i + 1]);
            assert_eq!(lines[i + 1].spans[0].style.fg, Some(Color::Blue));
        }
    }

    fn render(app: &App, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    fn render_buffer(app: &App, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn draw_renders_family_chrome() {
        let dir = TempDir::new().unwrap();
        let app = test_app(&dir, &["A", "B", "C"]);
        let rows = render(&app, 100, 30);
        assert!(rows[0].starts_with("╭─‹ bouncer ›─"), "{}", rows[0]);
        assert!(rows[0].ends_with("‹ never synced ›─╮"), "{}", rows[0]);
        let version = format!("‹ v{} ›─╯", env!("CARGO_PKG_VERSION"));
        assert!(rows[29].ends_with(&version), "{}", rows[29]);
        assert!(rows[1].contains("all 3   ·   » queued 0"), "{}", rows[1]);
        assert!(rows[3].starts_with("│ search: "), "{}", rows[3]);
        assert!(rows[5].contains("GETTING STARTED ─"), "{}", rows[5]);
        let rule = rows[27].trim_matches(|c| c == '│' || c == ' ');
        assert!(
            !rule.is_empty() && rule.chars().all(|c| c == '─'),
            "{}",
            rows[27]
        );
        assert!(
            rows[28].contains("type search · tab filter · ^r sync · esc quit"),
            "{}",
            rows[28]
        );
        let buf = render_buffer(&app, 100, 30);
        assert_eq!(buf[(0, 0)].fg, Color::Magenta);
    }

    #[test]
    fn draw_shows_results_header_with_count_while_searching() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["Hollow Knight", "Hades", "Celeste"]);
        app.input = "hol".into();
        app.cursor_pos = 3;
        let rows = render(&app, 100, 30);
        assert!(rows[5].contains("RESULTS ─"), "{}", rows[5]);
        assert!(
            rows[5].trim_end_matches(['│', ' ']).ends_with(" of 3"),
            "{}",
            rows[5]
        );
        assert!(rows[6].starts_with("│ │"), "{}", rows[6]);
        assert!(rows[6].contains("Hollow Knight"), "{}", rows[6]);
        assert!(
            rows[6].trim_end_matches(['│', ' ']).ends_with("steam"),
            "{}",
            rows[6]
        );
        assert!(rows[28].contains("enter queue"), "{}", rows[28]);
    }

    #[test]
    fn draw_colors_the_queued_view_cyan_and_ranks_its_rows() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C"]);
        queue_all(&mut app, &["B", "A"]);
        app.filter = Filter::Queued;
        let rows = render(&app, 100, 30);
        assert!(rows[5].contains("QUEUE ─"), "{}", rows[5]);
        assert!(
            rows[5].trim_end_matches(['│', ' ']).ends_with("2 games"),
            "{}",
            rows[5]
        );
        assert!(rows[6].starts_with("│ │1  »  B"), "{}", rows[6]);
        assert!(rows[7].starts_with("│ │2  »  A"), "{}", rows[7]);
        assert!(rows[28].contains("enter unqueue"), "{}", rows[28]);
        assert!(rows[28].contains("^j/^k rank"), "{}", rows[28]);
        let buf = render_buffer(&app, 100, 30);
        assert_eq!(buf[(2, 5)].fg, Color::Cyan);
        assert_eq!(buf[(2, 6)].fg, Color::Cyan);
    }

    #[test]
    fn draw_puts_queue_errors_in_the_status_line() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("queue.json"), "{ not json").unwrap();
        let app = test_app(&dir, &["A"]);
        let buf = render_buffer(&app, 100, 30);
        let row: String = (0..100)
            .map(|x| buf[(x, 26)].symbol().to_string())
            .collect();
        assert!(!row.trim_matches(['│', ' ']).is_empty(), "status row empty");
        assert_eq!(buf[(2, 26)].fg, Color::Red);
    }

    #[test]
    fn draw_keeps_the_highlight_on_a_selection_below_the_fold() {
        let dir = TempDir::new().unwrap();
        let names: Vec<String> = (0..40).map(|i| format!("Game {i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = test_app(&dir, &refs);
        app.input = "game".into();
        app.cursor_pos = 4;
        app.selected = 35;
        let expected = search::fuzzy_search(&app.input, &app.filtered())[35]
            .game
            .name
            .clone();
        let buf = render_buffer(&app, 100, 30);
        let highlighted: Vec<String> = (0..30)
            .filter(|&y| buf[(3, y)].bg == Color::DarkGray)
            .map(|y| (0..100).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect();
        assert_eq!(highlighted.len(), 1, "{highlighted:#?}");
        assert!(highlighted[0].contains(&expected), "{}", highlighted[0]);
        assert_ne!(
            buf[(2, 6)].bg,
            Color::DarkGray,
            "gutter stays out of the bar"
        );
    }

    #[test]
    fn draw_survives_tiny_terminals() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D", "E"]);
        queue_all(&mut app, &["A", "B", "C", "D", "E"]);
        for (w, h) in [(40, 12), (20, 8), (100, 10)] {
            render(&app, w, h);
        }
    }

    #[test]
    fn filter_row_colors_only_the_active_filter() {
        let cases: &[(Filter, &str, Color)] = &[
            (Filter::All, "all", Color::Magenta),
            (Filter::Queued, "queued", Color::Cyan),
            (Filter::Played, "played", Color::Green),
            (Filter::Unplayed, "unplayed", Color::Yellow),
        ];
        let labels = ["all", "queued", "played", "unplayed"];
        let mut misses = Vec::new();
        for &(active, label, color) in cases {
            let line = filter_row(active, [1543, 4, 2, 1541]);
            let text = line_text(&line);
            if text != "all 1543   ·   » queued 4   ·   ✓ played 2   ·   unplayed 1541" {
                misses.push(format!("{active:?}: {text}"));
            }
            let span = line.spans.iter().find(|s| s.content == label).unwrap();
            if span.style.fg != Some(color) || !span.style.add_modifier.contains(Modifier::BOLD) {
                misses.push(format!("{active:?}: active label style {:?}", span.style));
            }
            for other in line
                .spans
                .iter()
                .filter(|s| labels.contains(&s.content.as_ref()) && s.content != label)
            {
                if other.style.fg != Some(Color::DarkGray) {
                    misses.push(format!(
                        "{active:?}: inactive {} is {:?}",
                        other.content, other.style.fg
                    ));
                }
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }

    #[test]
    fn section_header_fills_width_with_rule_and_right_aligned_tail() {
        let l = section_header("RESULTS", Color::Magenta, Some("11 of 1543"), 40);
        let t = line_text(&l);
        assert_eq!(t.chars().count(), 40);
        assert!(t.starts_with("RESULTS ─"), "{t}");
        assert!(t.ends_with("─ 11 of 1543"), "{t}");
        assert_eq!(l.spans[0].style.fg, Some(Color::Magenta));
        assert!(l.spans[0].style.add_modifier.contains(Modifier::BOLD));
        let bare = line_text(&section_header("GETTING STARTED", Color::Blue, None, 30));
        assert_eq!(bare.chars().count(), 30);
        assert!(bare.ends_with('─'), "{bare}");
        let narrow = line_text(&section_header(
            "GETTING STARTED",
            Color::Blue,
            Some("x"),
            5,
        ));
        assert!(narrow.chars().count() <= 15, "{narrow}");
    }

    #[test]
    fn styled_hint_grays_keys_and_dims_descriptions() {
        let l = styled_hint("↑↓ move · esc quit");
        assert_eq!(line_text(&l), "↑↓ move · esc quit");
        let fg = |c: &str| l.spans.iter().find(|s| s.content == c).unwrap().style.fg;
        assert_eq!(fg("↑↓"), Some(Color::Gray));
        assert_eq!(fg(" move"), Some(Color::DarkGray));
        assert_eq!(fg(" · "), Some(Color::DarkGray));
        assert_eq!(fg("esc"), Some(Color::Gray));
    }

    #[test]
    fn hint_text_only_names_keys_that_apply() {
        const CASES: &[(Filter, bool, bool, &str)] = &[
            (
                Filter::All,
                false,
                false,
                "type search · tab filter · ^r sync · esc quit",
            ),
            (
                Filter::Queued,
                false,
                false,
                "type search · tab filter · ^r sync · esc quit",
            ),
            (
                Filter::All,
                true,
                false,
                "↑↓ move · enter queue · ^p played · tab filter · esc quit",
            ),
            (
                Filter::All,
                true,
                true,
                "↑↓ move · enter unqueue · ^p played · tab filter · esc quit",
            ),
            (
                Filter::Played,
                true,
                false,
                "↑↓ move · enter queue · ^p played · tab filter · esc quit",
            ),
            (
                Filter::Queued,
                true,
                true,
                "↑↓ move · enter unqueue · ^p played · ^j/^k rank · tab filter · esc quit",
            ),
        ];
        let misses: Vec<_> = CASES
            .iter()
            .filter(|(f, r, q, want)| hint_text(*f, *r, *q) != *want)
            .collect();
        assert!(misses.is_empty(), "{misses:?}");
    }

    #[test]
    fn an_unreadable_queue_file_is_surfaced_and_never_overwritten() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("queue.json");
        std::fs::write(&path, "{ not json at all").unwrap();

        let mut app = test_app(&dir, &["Tunic"]);
        assert!(app.status_message.is_some());
        assert!(!app.queue_writable);

        app.input = "tunic".to_string();
        app.toggle_queued();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ not json at all",
            "a queue file we could not read must survive a toggle"
        );
    }
}
