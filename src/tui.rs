use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph};

use crate::LibraryEntry;
use crate::loader::{self, LoadResult};
use crate::queue::{Filter, PLAYED_GLYPH, QUEUED_GLYPH, apply_filter};
use crate::search;
use crate::sync;
use crate::sync::{SyncReport, SyncStatus};

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
    cache_dir: PathBuf,
    heroic_dir: PathBuf,
    config_path: PathBuf,
    queue: crate::queue::Queue,
    queue_path: PathBuf,
    /// False when the queue file failed to load, so a file we could not read
    /// is never replaced by the empty queue standing in for it.
    queue_writable: bool,
    filter: crate::queue::Filter,
    /// The game last toggled, kept visible under the active filter even after
    /// it stops matching, until the cursor moves, the tab or search changes,
    /// or a sync reloads the library. Without it a toggled row vanishes from
    /// under the cursor.
    pinned: Option<String>,
    /// True when the pinned game was pinned by being unqueued, so queueing it
    /// again before the pin is released undoes the unqueue in its old rank
    /// rather than sending it to the back of the queue.
    pinned_unqueued: bool,
    /// Queue load or write failure, shown in place of the hint until the next
    /// successful write.
    status_message: Option<String>,
    /// The details popup's checkbox cursor: `None` when the popup is closed,
    /// `0` on the queued row, `1` on the played row.
    details_cursor: Option<usize>,
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
            cache_dir,
            heroic_dir,
            config_path,
            queue,
            queue_path,
            queue_writable,
            filter: crate::queue::Filter::default(),
            pinned: None,
            pinned_unqueued: false,
            status_message,
            details_cursor: None,
        }
    }

    /// The library narrowed to the active filter, in the order it renders.
    fn filtered(&self) -> Vec<LibraryEntry> {
        crate::queue::apply_filter_keeping(
            &self.games,
            &self.queue,
            self.filter,
            self.pinned.as_deref(),
        )
    }

    /// The pinned game, if it no longer matches the active filter and is only
    /// on screen because it is pinned. Only this row is styled as leaving: a
    /// pinned game that still matches is not going anywhere.
    fn leaving_game(&self) -> Option<String> {
        let pinned = self.pinned.as_deref()?;
        let matching = crate::queue::apply_filter(&self.games, &self.queue, self.filter);
        (!matching.iter().any(|g| g.name == pinned)).then(|| pinned.to_string())
    }

    /// Clears the pinned row and moves the cursor to the top, for any change
    /// that replaces the visible rows wholesale.
    fn reset_rows(&mut self) {
        self.pinned = None;
        self.pinned_unqueued = false;
        self.selected = 0;
    }

    /// Moves the cursor one row, releasing the pinned row.
    ///
    /// The cursor lands on the game it was heading for, looked up in the rows
    /// left once the pin is gone, so a pinned row dropping out above the
    /// cursor does not make it skip a game.
    fn move_cursor(&mut self, down: bool) {
        let target = self.selected_row().map(|(current, _)| {
            let filtered = self.filtered();
            let rows = search::fuzzy_search(&self.input, &filtered);
            let next = if down {
                (current + 1).min(rows.len() - 1)
            } else {
                current.saturating_sub(1)
            };
            (next, rows[next].game.name.clone())
        });
        self.pinned = None;
        self.pinned_unqueued = false;
        let Some((next, name)) = target else {
            return;
        };
        let filtered = self.filtered();
        self.selected = search::fuzzy_search(&self.input, &filtered)
            .iter()
            .position(|r| r.game.name == name)
            .unwrap_or(next);
    }

    fn start_sync(&mut self) {
        if matches!(self.sync_state, SyncState::Syncing) {
            return;
        }
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
        self.reload_library(load_result);
        self.sync_state = SyncState::Done(reports);
        self.sync_rx = None;
    }

    /// Swaps in a freshly loaded library. An open details popup follows its
    /// game to the game's new row, or closes when the game is gone.
    fn reload_library(&mut self, load_result: LoadResult) {
        let open_game = self.details_cursor.and(self.selected_game());
        self.games = load_result.games;
        self.sync_age = loader::format_sync_age(load_result.oldest_update);
        self.reset_rows();
        let Some(name) = open_game else {
            return;
        };
        if !self.games.iter().any(|g| g.name == name) {
            self.details_cursor = None;
            return;
        }
        self.pinned = Some(name.clone());
        let row = search::fuzzy_search(&self.input, &self.filtered())
            .iter()
            .position(|r| r.game.name == name);
        match row {
            Some(row) => self.selected = row,
            None => self.details_cursor = None,
        }
    }

    /// Opens the details popup on the selected game; does nothing with no rows.
    fn open_details(&mut self) {
        if self.selected_game().is_some() {
            self.details_cursor = Some(0);
        }
    }

    /// Handles a key while the details popup is open. The search box is not
    /// listening, so unmapped keys are ignored rather than typed.
    fn handle_details_key(&mut self, key: KeyEvent) {
        let plain_or_ctrl = matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::CONTROL);
        let cursor = self.details_cursor.unwrap_or(0);
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.details_cursor = None,
            KeyCode::Down | KeyCode::Tab => self.details_cursor = Some(1),
            KeyCode::Up | KeyCode::BackTab => self.details_cursor = Some(0),
            KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE => {
                if cursor == 0 {
                    self.toggle_queued();
                } else {
                    self.toggle_played();
                }
            }
            KeyCode::Char('q') if plain_or_ctrl => self.toggle_queued(),
            KeyCode::Char('p') if plain_or_ctrl => self.toggle_played(),
            _ => {}
        }
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
        if self.pinned_unqueued && self.pinned.as_deref() == Some(name.as_str()) {
            self.queue.restore_queued(&name);
            self.pinned_unqueued = false;
        } else {
            self.pinned_unqueued = self.queue.state(&name).queued;
            self.queue.toggle_queued(&name);
        }
        self.pinned = Some(name);
        self.persist_queue();
    }

    fn toggle_played(&mut self) {
        let Some(name) = self.selected_game() else {
            return;
        };
        self.queue.toggle_played(&name);
        self.pinned = Some(name);
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
            if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                app.quit = true;
            } else if app.details_cursor.is_some() {
                app.handle_details_key(key);
            } else {
                handle_list_key(app, key);
            }
        }
    }
}

/// Handles a key while the result list and search box have focus.
fn handle_list_key(app: &mut App, key: KeyEvent) {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.quit = true;
        }
        (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
            app.start_sync();
        }
        (KeyCode::BackTab, _) => {
            app.filter = app.filter.prev();
            app.reset_rows();
        }
        (KeyCode::Tab, _) => {
            app.filter = app.filter.next();
            app.reset_rows();
        }
        (KeyCode::Enter, _) => {
            app.open_details();
        }
        (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
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
            app.reset_rows();
        }
        (KeyCode::Backspace, _) if app.cursor_pos > 0 => {
            let prev = app.input[..app.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            app.input.drain(prev..app.cursor_pos);
            app.cursor_pos = prev;
            app.reset_rows();
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
            app.move_cursor(true);
        }
        (KeyCode::Up, _) => {
            app.move_cursor(false);
        }
        _ => {}
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
/// row keys only when rows exist, ranking only under the queued filter.
fn hint_text(filter: Filter, has_rows: bool) -> &'static str {
    match (has_rows, filter == Filter::Queued) {
        (false, _) => "type search · tab filter · ^r sync · esc quit",
        (true, true) => {
            "↑↓ move · enter details · ^q queue · ^p played · ^j/^k rank · tab filter · esc quit"
        }
        (true, false) => "↑↓ move · enter details · ^q queue · ^p played · tab filter · esc quit",
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

fn format_sync_summary(reports: &[SyncReport]) -> String {
    reports
        .iter()
        .map(|r| {
            let value = match &r.status {
                SyncStatus::Ok => {
                    let delta = r.added.len() as i64 - r.removed.len() as i64;
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
            key("enter", vec![text("see what you can do with a game")]),
            key(
                "^q",
                vec![
                    text("add it to your queue "),
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

    // A queue problem outranks the sync summary: it is the only state here the
    // user cannot regenerate by syncing again.
    let status = match (&app.status_message, &app.sync_state) {
        (Some(message), _) => Span::styled(message.clone(), Style::default().fg(Color::Red)),
        (None, SyncState::Syncing) => Span::styled("syncing...", dim),
        (None, SyncState::Done(reports)) => Span::styled(format_sync_summary(reports), dim),
        (None, SyncState::Idle) => Span::raw(""),
    };
    f.render_widget(Paragraph::new(Line::from(status)), status_area);
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(rule_area.width as usize), dim)),
        rule_area,
    );
    f.render_widget(
        Paragraph::new(styled_hint(hint_text(app.filter, !results.is_empty()))),
        hint_area,
    );

    if show_empty_state {
        let mut lines = empty_state_lines(app, body_area.width);
        let header = lines.remove(0);
        f.render_widget(Paragraph::new(header), header_area);
        f.render_widget(Paragraph::new(lines), body_area);
        return;
    }

    let leaving_game = app.leaving_game();
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
        let selected_row = i + scroll_offset == selected;
        let leaving = leaving_game.as_deref() == Some(result.game.name.as_str());
        if selected_row || leaving {
            // Dim replaces bold on a leaving row: many terminals cannot show both.
            let emphasis = if leaving {
                Modifier::DIM
            } else {
                Modifier::BOLD
            };
            for span in spans.iter_mut().skip(1) {
                if selected_row {
                    span.style = span.style.bg(Color::DarkGray);
                }
                span.style = span.style.add_modifier(emphasis);
            }
        }
        let row_area = Rect::new(body_area.x, body_area.y + i as u16, body_area.width, 1);
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }

    if app.details_cursor.is_some() {
        draw_details(f, app, body_area);
    }
}

/// Formats an RFC3339 timestamp as `Nov 27, 2026` in local time, or `None`
/// when it does not parse.
fn format_date(rfc3339: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    Some(
        parsed
            .with_timezone(&chrono::Local)
            .format("%b %-d, %Y")
            .to_string(),
    )
}

/// Draws the game details popup centered over `area`, clamped to fit it.
fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let Some(name) = app.selected_game() else {
        return;
    };
    let entry = app.games.iter().find(|g| g.name == name);
    let state = app.queue.entry(&name);
    let queued = state.is_some_and(|e| e.queued);
    let played = state.is_some_and(|e| e.played);
    let dim = Style::default().fg(Color::DarkGray);
    let chord = Style::default().fg(Color::Gray);

    let width = area.width.min(46);
    let inner_width = width.saturating_sub(2) as usize;
    let platforms: &[String] = entry.map_or(&[], |g| g.platforms.as_slice());
    let rank = if queued { app.queue.rank(&name) } else { None };

    let flag_row = |index: usize,
                    on: bool,
                    glyph: char,
                    color: Color,
                    label: &str,
                    at: Option<&String>,
                    keys: &str| {
        let date = if on {
            at.and_then(|d| format_date(d))
        } else {
            None
        };
        let mut text_len = 2 + 3 + 1 + 1 + label.chars().count() + 1;
        let mut spans = vec![
            Span::raw("  "),
            Span::raw(if on { "[x] " } else { "[ ] " }),
            Span::styled(glyph.to_string(), Style::default().fg(color)),
            Span::raw(format!(" {label}")),
        ];
        if let Some(date) = date {
            text_len += 2 + date.chars().count();
            spans.push(Span::styled(format!("  {date}"), dim));
        }
        spans.push(Span::raw(
            " ".repeat(inner_width.saturating_sub(text_len + 4)),
        ));
        spans.push(Span::styled(keys.to_string(), chord));
        let mut line = Line::from(spans);
        if app.details_cursor == Some(index) {
            line = line.patch_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        }
        line
    };

    let mut lines = vec![
        Line::raw(""),
        flag_row(
            0,
            queued,
            QUEUED_GLYPH,
            Color::Cyan,
            "queued",
            state.and_then(|e| e.queued_at.as_ref()),
            "^q",
        ),
        flag_row(
            1,
            played,
            PLAYED_GLYPH,
            Color::Green,
            "played",
            state.and_then(|e| e.played_at.as_ref()),
            "^p",
        ),
        Line::raw(""),
    ];
    for (i, platform) in platforms.iter().enumerate() {
        let label = if i == 0 { "owned on  " } else { "          " };
        let mut spans = vec![
            Span::styled(format!("  {label}"), dim),
            Span::styled(
                platform.clone(),
                Style::default()
                    .fg(platform_color(platform))
                    .add_modifier(Modifier::ITALIC),
            ),
        ];
        if let Some(date) = entry
            .and_then(|g| g.first_seen.get(platform))
            .and_then(|d| format_date(d))
        {
            spans.push(Span::styled(format!("   {date}"), dim));
        }
        lines.push(Line::from(spans));
    }
    if let Some(rank) = rank {
        lines.push(Line::from(vec![
            Span::styled("  rank      ", dim),
            Span::raw(format!(
                "#{rank} of {}",
                app.queue.entries.iter().filter(|e| e.queued).count()
            )),
        ]));
    }
    lines.push(Line::styled(
        format!(" {}", "─".repeat(inner_width.saturating_sub(2))),
        dim,
    ));
    let mut hint = styled_hint("↑↓ move · space toggle · esc close");
    hint.spans.insert(0, Span::raw("  "));
    lines.push(hint);

    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );

    let max_title = inner_width.saturating_sub(6);
    let title = if name.chars().count() > max_title {
        let kept: String = name.chars().take(max_title.saturating_sub(1)).collect();
        format!("{kept}…")
    } else {
        name
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(corner_title(&title, true, true));
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(block), popup);
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
                    ..Default::default()
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

    fn ok(platform: &str, added: usize, removed: usize) -> SyncReport {
        SyncReport {
            added: vec![String::new(); added],
            removed: vec![String::new(); removed],
            ..SyncReport::without_games(platform, SyncStatus::Ok)
        }
    }

    #[test]
    fn sync_summary_renders_each_platform() {
        let cases: Vec<(Vec<SyncReport>, &str)> = vec![
            (
                vec![ok("steam", 42, 0), ok("epic", 10, 0)],
                "steam +42  epic +10",
            ),
            (
                vec![ok("steam", 3, 0), ok("epic", 0, 0)],
                "steam +3  epic +0",
            ),
            (vec![ok("steam", 0, 2)], "steam -2"),
            (vec![ok("steam", 2, 2)], "steam +0"),
            (
                vec![
                    ok("steam", 42, 0),
                    SyncReport::without_games(
                        "gog",
                        SyncStatus::Skipped("gog_library.json not found".to_string()),
                    ),
                    SyncReport::without_games(
                        "amazon",
                        SyncStatus::Error("network error".to_string()),
                    ),
                ],
                "steam +42  gog skip  amazon error",
            ),
        ];
        let misses: Vec<String> = cases
            .iter()
            .filter_map(|(reports, want)| {
                let got = format_sync_summary(reports);
                (got != *want).then(|| format!("want {want:?}, got {got:?}"))
            })
            .collect();
        assert!(misses.is_empty(), "\n{}", misses.join("\n"));
    }

    #[test]
    fn empty_reports_produce_empty_string() {
        assert_eq!(format_sync_summary(&[]), "");
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

    fn visible(app: &App) -> Vec<String> {
        search::fuzzy_search(&app.input, &app.filtered())
            .iter()
            .map(|r| r.game.name.clone())
            .collect()
    }

    #[test]
    fn a_game_toggled_out_of_the_filter_stays_until_the_cursor_moves() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C"]);
        app.filter = crate::queue::Filter::Unplayed;
        app.selected = 1;

        app.toggle_played();
        assert!(app.queue.state("B").played);
        assert_eq!(
            visible(&app),
            ["A", "B", "C"],
            "B should linger after toggling"
        );

        app.move_cursor(true);
        assert_eq!(visible(&app), ["A", "C"]);
        assert_eq!(
            app.selected_game(),
            Some("C".to_string()),
            "cursor must not skip C"
        );
    }

    #[test]
    fn moving_up_off_a_pinned_row_lands_on_the_row_above() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C"]);
        app.filter = crate::queue::Filter::Unplayed;
        app.selected = 1;

        app.toggle_played();
        app.move_cursor(false);
        assert_eq!(app.selected_game(), Some("A".to_string()));
    }

    #[test]
    fn toggling_twice_without_moving_keeps_the_row_and_restores_state() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.filter = crate::queue::Filter::Unplayed;
        app.selected = 0;

        app.toggle_played();
        app.toggle_played();
        assert!(!app.queue.state("A").played);
        assert_eq!(app.selected_game(), Some("A".to_string()));
    }

    #[test]
    fn undoing_an_unqueue_before_moving_keeps_the_old_rank() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C"]);
        queue_all(&mut app, &["A", "B", "C"]);
        app.filter = crate::queue::Filter::Queued;
        app.selected = 0;

        app.toggle_queued();
        assert_eq!(app.queue.rank("A"), None);
        app.toggle_queued();
        assert_eq!(app.queue.rank("A"), Some(1), "undo should restore rank 1");
        app.toggle_queued();
        app.toggle_queued();
        assert_eq!(app.queue.rank("A"), Some(1), "repeated undo should too");
    }

    #[test]
    fn requeueing_after_moving_away_goes_to_the_back() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C"]);
        queue_all(&mut app, &["A", "B", "C"]);
        app.input = "a".to_string();
        app.toggle_queued();
        app.reset_rows();
        app.input = "a".to_string();

        app.toggle_queued();
        assert_eq!(app.queue.rank("A"), Some(3));
    }

    #[test]
    fn queueing_a_game_just_marked_played_still_goes_to_the_back() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.input = "a".to_string();
        app.toggle_played();
        app.queue.toggle_queued("B");

        app.toggle_queued();
        assert_eq!(app.queue.rank("A"), Some(2));
    }

    #[test]
    fn only_a_pinned_game_that_left_the_filter_counts_as_leaving() {
        use crate::queue::Filter;
        // (filter, action, leaving?) for game "A" in a library of A and B.
        const CASES: &[(Filter, &str, bool)] = &[
            (Filter::Unplayed, "played", true),
            (Filter::Unplayed, "queued", false),
            (Filter::All, "played", false),
            (Filter::All, "queued", false),
            (Filter::Played, "played", true),
            (Filter::Queued, "queued", true),
            (Filter::Queued, "played", false),
        ];
        let misses: Vec<String> = CASES
            .iter()
            .filter_map(|&(filter, action, want)| {
                let dir = TempDir::new().unwrap();
                let mut app = test_app(&dir, &["A", "B"]);
                app.queue.toggle_queued("A");
                if filter == Filter::Played {
                    app.queue.toggle_played("A");
                }
                app.filter = filter;
                app.input = "a".to_string();
                match action {
                    "played" => app.toggle_played(),
                    _ => app.toggle_queued(),
                }
                let got = app.leaving_game().is_some();
                (got != want).then(|| format!("{filter:?} after {action}: leaving={got}"))
            })
            .collect();
        assert!(misses.is_empty(), "{misses:#?}");
    }

    #[test]
    fn switching_tabs_or_typing_releases_the_pinned_row() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.filter = crate::queue::Filter::Unplayed;
        app.toggle_played();
        app.reset_rows();
        assert_eq!(visible(&app), ["B"]);
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
            ("enter", "see what you can do with a game"),
            ("^q", "add it to your queue \u{bb}"),
            ("^p", "mark it played \u{2713}"),
            ("tab", "switch between all / queued / played / unplayed"),
        ];
        assert!(t[0].starts_with("GETTING STARTED ─"), "{}", t[0]);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Blue));
        assert_eq!(t.len(), 6);
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
        assert!(rows[28].contains("enter details"), "{}", rows[28]);
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
        assert!(rows[28].contains("enter details"), "{}", rows[28]);
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
        const ROWS: &str = "↑↓ move · enter details · ^q queue · ^p played";
        const NO_ROWS: &str = "type search · tab filter · ^r sync · esc quit";
        let cases = [
            (Filter::All, false, NO_ROWS.to_string()),
            (Filter::Queued, false, NO_ROWS.to_string()),
            (Filter::All, true, format!("{ROWS} · tab filter · esc quit")),
            (
                Filter::Played,
                true,
                format!("{ROWS} · tab filter · esc quit"),
            ),
            (
                Filter::Queued,
                true,
                format!("{ROWS} · ^j/^k rank · tab filter · esc quit"),
            ),
        ];
        let misses: Vec<_> = cases
            .iter()
            .filter(|(f, r, want)| hint_text(*f, *r) != *want)
            .collect();
        assert!(misses.is_empty(), "{misses:?}");
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn details_keys() {
        use KeyCode::*;
        // (keys, want_open, want_cursor, want_queued, want_played)
        let cases: Vec<(Vec<KeyEvent>, bool, usize, bool, bool)> = vec![
            (vec![key(Esc)], false, 0, false, false),
            (vec![key(Enter)], false, 0, false, false),
            (vec![key(Down)], true, 1, false, false),
            (vec![key(Down), key(Down)], true, 1, false, false),
            (vec![key(Down), key(Up)], true, 0, false, false),
            (vec![key(Tab)], true, 1, false, false),
            (vec![key(Tab), key(BackTab)], true, 0, false, false),
            (vec![key(Char(' '))], true, 0, true, false),
            (vec![key(Down), key(Char(' '))], true, 1, false, true),
            (vec![key(Char('q'))], true, 0, true, false),
            (vec![ctrl('q')], true, 0, true, false),
            (vec![key(Char('p'))], true, 0, false, true),
            (vec![ctrl('p')], true, 0, false, true),
            (vec![key(Char('q')), key(Char('q'))], true, 0, false, false),
            (vec![key(Char('x'))], true, 0, false, false),
            (vec![ctrl('j'), ctrl('k')], true, 0, false, false),
        ];
        let mut misses = Vec::new();
        for (i, (keys, open, cursor, queued, played)) in cases.iter().enumerate() {
            let dir = TempDir::new().unwrap();
            let mut app = test_app(&dir, &["A", "B"]);
            app.filter = Filter::Unplayed;
            app.open_details();
            for k in keys {
                app.handle_details_key(*k);
            }
            let state = app.queue.state("A");
            let got = (
                app.details_cursor.is_some(),
                app.details_cursor.unwrap_or(0),
                state.queued,
                state.played,
                app.input.is_empty(),
            );
            let want = (
                *open,
                if *open { *cursor } else { 0 },
                *queued,
                *played,
                true,
            );
            if got != want {
                misses.push(format!("row {i}: got {got:?}, want {want:?}"));
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }

    #[test]
    fn list_keys_toggle_and_open() {
        use KeyCode::*;
        // (input, keys, want_open, want_queued, want_played)
        let cases: Vec<(&str, Vec<KeyEvent>, bool, bool, bool)> = vec![
            ("a", vec![ctrl('q')], false, true, false),
            ("a", vec![ctrl('p')], false, false, true),
            ("a", vec![key(Enter)], true, false, false),
            ("zzzz", vec![key(Enter)], false, false, false),
        ];
        let mut misses = Vec::new();
        for (i, (input, keys, open, queued, played)) in cases.iter().enumerate() {
            let dir = TempDir::new().unwrap();
            let mut app = test_app(&dir, &["A", "B"]);
            app.input = input.to_string();
            for k in keys {
                handle_list_key(&mut app, *k);
            }
            let state = app.queue.state("A");
            let got = (app.details_cursor == Some(0), state.queued, state.played);
            if got != (*open, *queued, *played) {
                misses.push(format!("row {i}: got {got:?}"));
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }

    fn library(names: &[&str]) -> LoadResult {
        LoadResult {
            games: names
                .iter()
                .map(|n| LibraryEntry {
                    name: n.to_string(),
                    platforms: vec!["steam".to_string()],
                    ..Default::default()
                })
                .collect(),
            warnings: Vec::new(),
            oldest_update: None,
        }
    }

    #[test]
    fn details_closes_when_game_leaves_library() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.filter = Filter::Unplayed;
        app.open_details();
        app.reload_library(library(&["B"]));
        assert_eq!(app.details_cursor, None);
    }

    #[test]
    fn details_follows_its_game_across_a_reload() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.filter = Filter::Unplayed;
        app.selected = 1;
        app.open_details();
        app.reload_library(library(&["A", "B", "C"]));
        assert_eq!(app.details_cursor, Some(0));
        assert_eq!(app.selected_game(), Some("B".to_string()));
    }

    #[test]
    fn details_stays_open_after_reload_when_filter_hides_its_game() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.filter = Filter::Unplayed;
        app.open_details();
        app.handle_details_key(key(KeyCode::Char('p')));
        app.reload_library(library(&["A", "B"]));
        assert_eq!(app.details_cursor, Some(0));
        assert_eq!(app.selected_game(), Some("A".to_string()));
    }

    #[test]
    fn details_played_toggle_keeps_row_under_unplayed() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B"]);
        app.filter = Filter::Unplayed;
        app.open_details();
        app.handle_details_key(key(KeyCode::Char('p')));
        app.handle_details_key(key(KeyCode::Esc));
        assert_eq!(visible(&app), ["A", "B"]);
        handle_list_key(&mut app, key(KeyCode::Down));
        assert_eq!(visible(&app), ["B"]);
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

    #[test]
    fn format_date_rows() {
        let ok = format_date("2026-11-27T15:00:00+00:00").unwrap();
        assert!(ok.contains("2026") && ok.starts_with("Nov 2"), "{ok}");
        assert_eq!(format_date("garbage"), None);
    }

    fn popup_app(dir: &TempDir, name: &str) -> App {
        let mut app = test_app(dir, &[name]);
        app.games[0].platforms = vec!["steam".to_string(), "gog".to_string()];
        app.games[0]
            .first_seen
            .insert("steam".to_string(), "2026-03-04T12:00:00+00:00".to_string());
        app.filter = Filter::Unplayed;
        app.open_details();
        app
    }

    fn popup_text(app: &App, w: u16, h: u16) -> String {
        render(app, w, h).join("\n")
    }

    #[test]
    fn details_popup_render_rows() {
        let dir = TempDir::new().unwrap();
        let mut closed = popup_app(&dir, "Hollow Knight");
        closed.details_cursor = None;
        let mut off = popup_app(&dir, "Hollow Knight");
        off.queue.toggle_queued("Hollow Knight");
        off.queue.toggle_queued("Hollow Knight");
        off.queue.entries[0].queued_at = Some("2026-11-27T15:00:00+00:00".to_string());
        let mut on = popup_app(&dir, "Hollow Knight");
        on.queue.toggle_queued("Hollow Knight");
        let plain = popup_app(&dir, "Hollow Knight");

        let always = [
            "‹ Hollow Knight ›",
            "[ ] ✓ played",
            "^q",
            "^p",
            "↑↓ move · space toggle · esc close",
            "owned on  steam   Mar 4, 2026",
        ];
        let cases: Vec<(&str, &App, Vec<&str>, Vec<&str>)> = vec![
            (
                "closed",
                &closed,
                vec![],
                vec!["↑↓ move · space", "owned on"],
            ),
            ("plain", &plain, always.to_vec(), vec!["rank", "[x]"]),
            (
                "queued",
                &on,
                vec!["[x] » queued", "2026", "#1 of 1", "rank"],
                vec![],
            ),
            (
                "flag off, stale date",
                &off,
                vec!["[ ] » queued"],
                vec!["rank", "Nov"],
            ),
        ];
        let mut misses = Vec::new();
        for (label, app, present, absent) in cases {
            let text = popup_text(app, 100, 30);
            for p in present {
                if !text.contains(p) {
                    misses.push(format!("{label}: missing {p:?}"));
                }
            }
            for a in absent {
                if text.contains(a) {
                    misses.push(format!("{label}: unexpected {a:?}"));
                }
            }
        }
        let text = popup_text(&plain, 100, 30);
        let gog = text.lines().find(|l| l.contains("gog")).unwrap();
        if gog.contains("202") {
            misses.push(format!("gog line has a date: {gog}"));
        }
        assert!(misses.is_empty(), "{misses:#?}\n{text}");
    }

    #[test]
    fn details_popup_survives_small_terminals_and_long_names() {
        let dir = TempDir::new().unwrap();
        let long = format!("{}™", "x".repeat(120));
        for (name, w, h) in [
            ("Hollow Knight", 50, 16),
            (long.as_str(), 100, 30),
            (long.as_str(), 50, 16),
        ] {
            let app = popup_app(&dir, name);
            let text = popup_text(&app, w, h);
            if name == long {
                assert!(text.contains("…"), "{text}");
            }
            assert!(text.lines().all(|l| l.chars().count() == w as usize));
        }
    }
}
