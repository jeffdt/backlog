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

const TAB_KEY_HINT: &str = "tab / shift-tab";

/// Draws the filter tabs as three rows of folder tabs: the active tab is open
/// to the content below, inactive tabs sit on a shared baseline. `counts` is
/// indexed in `FILTERS` order. The key hint is right-aligned when it fits.
fn tab_bar(active: Filter, counts: [usize; 4], width: u16) -> [Line<'static>; 3] {
    let width = width as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let mut top = vec![Span::raw(" ")];
    let mut mid = vec![Span::raw(" ")];
    let mut bot = vec![Span::styled("─", dim)];
    let mut used = 1;

    for (filter, count) in FILTERS.into_iter().zip(counts) {
        let is_active = filter == active;
        let border = if is_active {
            Style::default().fg(Color::White)
        } else {
            dim
        };
        let glyph = filter.glyph().map(|g| format!("{g} ")).unwrap_or_default();
        let glyph_color = match filter {
            Filter::Queued => Color::Cyan,
            Filter::Played => Color::Green,
            _ => Color::DarkGray,
        };
        let label = format!("{} {count}", filter.label());
        let inner = 2 + glyph.chars().count() + label.chars().count();

        top.push(Span::styled(format!("╭{}╮", "─".repeat(inner)), border));
        top.push(Span::raw(" "));
        mid.push(Span::styled("│ ", border));
        mid.push(Span::styled(glyph, Style::default().fg(glyph_color)));
        let label_style = if is_active {
            border.add_modifier(Modifier::BOLD)
        } else {
            border
        };
        mid.push(Span::styled(label, label_style));
        mid.push(Span::styled(" │", border));
        mid.push(Span::raw(" "));
        let (l, fill, r) = if is_active {
            ('╯', ' ', '╰')
        } else {
            ('┴', '─', '┴')
        };
        bot.push(Span::styled(
            format!("{l}{}{r}", fill.to_string().repeat(inner)),
            border,
        ));
        bot.push(Span::styled("─", dim));
        used += inner + 3;
    }

    let hint_len = TAB_KEY_HINT.chars().count();
    if width >= used + 2 + hint_len {
        mid.push(Span::raw(" ".repeat(width - used - hint_len)));
        mid.push(Span::styled(TAB_KEY_HINT, dim));
    } else {
        mid.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    }
    let pad = width.saturating_sub(used);
    top.push(Span::raw(" ".repeat(pad)));
    bot.push(Span::styled("─".repeat(pad), dim));
    [Line::from(top), Line::from(mid), Line::from(bot)]
}

/// The keybind hint for a filter; reordering is advertised only under the
/// queued filter, and only keys that act on a selected row appear when rows exist.
fn hint_text(filter: Filter, has_results: bool) -> String {
    match (has_results, filter) {
        (false, _) => "type to search".to_string(),
        (true, Filter::Queued) => "↑↓ select   enter queue   ^p played   ^j/^k rank".to_string(),
        (true, _) => "↑↓ select   enter queue   ^p played".to_string(),
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

/// One game row: optional rank, queue markers, name with match highlighting,
/// padding to `name_width`, then the colored platform list. Row background is
/// left to the caller.
fn result_row_spans(
    app: &App,
    name: &str,
    platforms: &[String],
    match_indices: &[u32],
    name_width: usize,
    rank: Option<usize>,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    if let Some(rank) = rank {
        spans.push(Span::styled(
            format!("{rank:>2} "),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let state = app.queue.state(name);
    spans.extend(marker_spans(state));
    let base_color = if state.played {
        Color::Gray
    } else {
        Color::White
    };
    spans.extend(name.chars().enumerate().map(|(char_idx, ch)| {
        let style = if match_indices.contains(&(char_idx as u32)) {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base_color)
        };
        Span::styled(ch.to_string(), style)
    }));

    let padding = name_width.saturating_sub(name.chars().count()) + 2;
    spans.push(Span::raw(" ".repeat(padding)));

    for (i, platform) in platforms.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" / ", Style::default().fg(Color::Gray)));
        }
        spans.push(Span::styled(
            platform.clone(),
            Style::default().fg(platform_color(platform)),
        ));
    }
    spans
}

const UP_NEXT_ROWS: usize = 3;

/// Body of the blank default screen: the top of the queue for returning
/// users, or a short key guide when the queue is empty.
fn empty_state_lines(app: &App) -> Vec<Line<'static>> {
    let header = |text: &'static str| {
        Line::from(Span::styled(
            text,
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let queued = apply_filter(&app.games, &app.queue, Filter::Queued);

    if queued.is_empty() {
        let key = |k: &str, desc: String| {
            Line::from(vec![
                Span::styled(format!("{k:<6}"), Style::default().fg(Color::Cyan)),
                Span::styled(desc, Style::default().fg(Color::Gray)),
            ])
        };
        return vec![
            header("getting started"),
            key(
                "type",
                "fuzzy-search every game you own, across all stores".into(),
            ),
            key(
                "enter",
                format!("add the selected game to your queue {QUEUED_GLYPH}"),
            ),
            key("^p", format!("mark it played {PLAYED_GLYPH}")),
            key(
                "tab",
                "switch between all / queued / played / unplayed".into(),
            ),
        ];
    }

    let shown = &queued[..queued.len().min(UP_NEXT_ROWS)];
    let name_width = shown
        .iter()
        .map(|e| e.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = vec![header("up next")];
    lines.extend(shown.iter().enumerate().map(|(i, e)| {
        Line::from(result_row_spans(
            app,
            &e.name,
            &e.platforms,
            &[],
            name_width,
            Some(i + 1),
        ))
    }));
    if queued.len() > shown.len() {
        lines.push(Line::from(Span::styled(
            format!(
                "+{} more \u{b7} tab to see the full queue",
                queued.len() - shown.len()
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let title = format!(" bouncer \u{2500}\u{2500} {} ", app.sync_age);

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .title_bottom(Line::from(" Esc to quit ").right_aligned())
        .padding(Padding::new(1, 1, 0, 0));

    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // blank
            Constraint::Length(3), // tab bar
            Constraint::Length(1), // blank
            Constraint::Length(3), // input box
            Constraint::Length(1), // blank
            Constraint::Min(1),    // body
            Constraint::Length(1), // hint / sync status
        ])
        .split(inner);

    let counts = view_counts(app);
    let active_count = counts[FILTERS.iter().position(|&x| x == app.filter).unwrap_or(0)];
    f.render_widget(
        Paragraph::new(tab_bar(app.filter, counts, chunks[1].width).to_vec()),
        chunks[1],
    );

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    let input_inner = input_block.inner(chunks[3]);
    f.render_widget(input_block, chunks[3]);
    let query_span = if app.input.is_empty() {
        Span::styled(
            format!("type to search {active_count} games"),
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::styled(app.input.clone(), Style::default().fg(Color::White))
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(">", Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            query_span,
        ])),
        input_inner,
    );
    let cursor_x = input_inner.x + 2 + app.input[..app.cursor_pos].chars().count() as u16;
    f.set_cursor_position((cursor_x, input_inner.y));

    let show_empty_state = app.input.is_empty() && app.filter == Filter::All;
    let filtered = app.filtered();
    let results = if show_empty_state {
        Vec::new()
    } else {
        search::fuzzy_search(&app.input, &filtered)
    };

    // A queue problem outranks the sync summary: it is the only state here the
    // user cannot regenerate by syncing again.
    let (hint, hint_color) = match &app.status_message {
        Some(message) => (message.clone(), Color::Red),
        None => {
            let hint = match &app.sync_state {
                SyncState::Idle => hint_text(app.filter, !results.is_empty()),
                SyncState::Syncing => "syncing...".to_string(),
                SyncState::Done(reports) => format_sync_summary(reports, &app.sync_before_counts),
            };
            (hint, Color::DarkGray)
        }
    };
    let status_line = Line::from(vec![Span::styled(hint, Style::default().fg(hint_color))]);
    f.render_widget(Paragraph::new(status_line), chunks[6]);

    let results_area = chunks[5];
    if show_empty_state {
        f.render_widget(Paragraph::new(empty_state_lines(app)), results_area);
        return;
    }
    if results.is_empty() {
        return;
    }

    let max_selected = results.len() - 1;
    let selected = app.selected.min(max_selected);
    let visible_rows = results_area.height as usize;

    let scroll_offset = if selected >= visible_rows {
        selected - visible_rows + 1
    } else {
        0
    };

    // Cap name length to leave room for the platform label
    let available_width = results_area.width as usize;
    let max_name_len = results
        .iter()
        .map(|r| r.game.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(available_width.saturating_sub(13));

    for (i, result) in results
        .iter()
        .skip(scroll_offset)
        .take(visible_rows)
        .enumerate()
    {
        let row_y = results_area.y + i as u16;
        if row_y >= results_area.y + results_area.height {
            break;
        }

        let abs_idx = i + scroll_offset;
        let row_area = Rect::new(results_area.x, row_y, results_area.width, 1);

        let rank = if app.filter == Filter::Queued {
            app.queue.rank(&result.game.name)
        } else {
            None
        };
        let spans = result_row_spans(
            app,
            &result.game.name,
            &result.game.platforms,
            &result.match_indices,
            max_name_len,
            rank,
        );

        let mut row_style = Style::default();
        if abs_idx == selected {
            row_style = row_style.bg(Color::DarkGray);
        } else if abs_idx % 2 == 0 {
            row_style = row_style.bg(Color::Black);
        }

        f.render_widget(Paragraph::new(Line::from(spans)).style(row_style), row_area);
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
    fn empty_state_shows_up_next_with_top_three_and_more_count() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D", "E"]);
        queue_all(&mut app, &["A", "B", "C", "D", "E"]);
        let t = texts(&empty_state_lines(&app));
        assert_eq!(t.len(), 5);
        assert!(t[0].contains("up next"));
        for (i, name) in ["A", "B", "C"].iter().enumerate() {
            assert!(t[i + 1].contains(&format!(" {} ", i + 1)), "{}", t[i + 1]);
            assert!(t[i + 1].contains(name));
        }
        assert_eq!(t[4].trim(), "+2 more \u{b7} tab to see the full queue");
    }

    #[test]
    fn empty_state_omits_more_line_for_three_or_fewer() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D"]);
        queue_all(&mut app, &["A", "B", "C"]);
        assert_eq!(empty_state_lines(&app).len(), 4);
    }

    #[test]
    fn empty_state_skips_queued_games_missing_from_library() {
        let dir = TempDir::new().unwrap();
        let mut app = test_app(&dir, &["A", "B", "C", "D"]);
        queue_all(&mut app, &["Gone", "A", "B", "C", "D"]);
        let t = texts(&empty_state_lines(&app));
        assert!(!t.iter().any(|l| l.contains("Gone")));
        assert_eq!(t.len(), 5);
        assert!(t[4].starts_with("+1 more"));
    }

    #[test]
    fn empty_state_shows_getting_started_when_queue_is_empty() {
        let dir = TempDir::new().unwrap();
        let app = test_app(&dir, &["A"]);
        let t = texts(&empty_state_lines(&app));
        let expected = [
            ("type", "fuzzy-search every game you own, across all stores"),
            ("enter", "add the selected game to your queue \u{bb}"),
            ("^p", "mark it played \u{2713}"),
            ("tab", "switch between all / queued / played / unplayed"),
        ];
        assert!(t[0].contains("getting started"));
        assert_eq!(t.len(), 5);
        for (i, (key, desc)) in expected.iter().enumerate() {
            assert!(t[i + 1].starts_with(key), "{}", t[i + 1]);
            assert!(t[i + 1].ends_with(desc), "{}", t[i + 1]);
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

    #[test]
    fn draw_renders_tabs_placeholder_and_title_without_game_count() {
        let dir = TempDir::new().unwrap();
        let app = test_app(&dir, &["A", "B", "C"]);
        let rows = render(&app, 100, 30);
        assert!(
            rows[0].contains("bouncer \u{2500}\u{2500} never synced"),
            "{}",
            rows[0]
        );
        assert!(!rows[0].contains("games ("));
        assert!(rows[2].contains("\u{256d}\u{2500}\u{2500}"), "{}", rows[2]);
        assert!(rows.iter().any(|r| r.contains("> type to search 3 games")));
        assert!(rows[28].contains("type to search"), "{}", rows[28]);
        assert!(!rows[28].contains(" all "));
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
    fn tab_bar_draws_folder_tabs_with_the_active_one_open() {
        let lines = tab_bar(Filter::All, [1543, 5, 12, 1531], 100);
        let [top, mid, bot] = lines.map(|l| line_text(&l));
        assert!(top.starts_with(" ╭──────────╮ ╭────────────╮"));
        assert!(mid.starts_with(" │ all 1543 │ │ » queued 5 │ │ ✓ played 12 │ │ unplayed 1531 │"));
        assert!(mid.trim_end().ends_with("tab / shift-tab"));
        assert!(bot.starts_with("─╯          ╰─┴────────────┴─┴"));
        for l in [&top, &mid, &bot] {
            assert_eq!(l.chars().count(), 100);
        }
    }

    #[test]
    fn tab_bar_opens_whichever_tab_is_active() {
        let bot = line_text(&tab_bar(Filter::Queued, [1543, 5, 12, 1531], 100)[2]);
        assert!(bot.starts_with("─┴──────────┴─╯            ╰─┴"));
    }

    #[test]
    fn tab_bar_drops_the_key_hint_when_too_narrow() {
        let lines = tab_bar(Filter::All, [1543, 5, 12, 1531], 70).map(|l| line_text(&l));
        assert!(!lines[1].contains("tab / shift-tab"));
        for l in &lines {
            assert_eq!(l.chars().count(), 70);
        }
    }

    #[test]
    fn tab_bar_colors_active_white_and_inactive_dark_gray() {
        let mid = &tab_bar(Filter::All, [1543, 5, 12, 1531], 100)[1];
        let fg = |content: &str| {
            mid.spans
                .iter()
                .find(|s| s.content == content)
                .unwrap_or_else(|| panic!("no span {content:?}"))
                .style
                .fg
        };
        let first_border = mid.spans.iter().find(|s| s.content == "│ ").unwrap();
        assert_eq!(first_border.style.fg, Some(Color::White));
        assert_eq!(fg("all 1543"), Some(Color::White));
        let active_label = mid.spans.iter().find(|s| s.content == "all 1543").unwrap();
        assert!(active_label.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(fg("queued 5"), Some(Color::DarkGray));
        assert_eq!(fg("» "), Some(Color::Cyan));
        assert_eq!(fg("✓ "), Some(Color::Green));
        assert_eq!(fg("tab / shift-tab"), Some(Color::DarkGray));
    }

    #[test]
    fn hint_text_only_names_keys_that_apply() {
        const CASES: &[(Filter, bool, &str)] = &[
            (Filter::All, false, "type to search"),
            (Filter::All, true, "↑↓ select   enter queue   ^p played"),
            (Filter::Played, true, "↑↓ select   enter queue   ^p played"),
            (
                Filter::Queued,
                true,
                "↑↓ select   enter queue   ^p played   ^j/^k rank",
            ),
            (Filter::Queued, false, "type to search"),
        ];
        let misses: Vec<_> = CASES
            .iter()
            .filter(|(f, r, want)| hint_text(*f, *r) != *want)
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
