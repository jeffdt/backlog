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
use crate::queue::{PLAYED_GLYPH, QUEUED_GLYPH};
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
    filter: crate::queue::Filter,
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
            queue: crate::queue::load(&queue_path),
            queue_path,
            filter: crate::queue::Filter::default(),
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

    /// The game under the cursor, resolved against the currently visible rows.
    fn selected_game(&self) -> Option<String> {
        let filtered = self.filtered();
        let results = search::fuzzy_search(&self.input, &filtered);
        let selected = self.selected.min(results.len().saturating_sub(1));
        results.get(selected).map(|r| r.game.name.clone())
    }

    /// Persists the queue, surfacing a write failure in the status row rather
    /// than tearing down the terminal.
    fn persist_queue(&mut self) {
        if let Err(e) = crate::queue::save(&self.queue_path, &self.queue) {
            self.sync_state = SyncState::Done(vec![SyncReport {
                platform: "queue".to_string(),
                game_count: 0,
                status: SyncStatus::Error(e.to_string()),
            }]);
        }
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
    /// Only meaningful under the queued filter. With a search active, rows are
    /// score-ordered rather than rank-ordered, so the cursor stays put.
    fn move_selected(&mut self, down: bool) {
        if self.filter != crate::queue::Filter::Queued {
            return;
        }
        let Some(name) = self.selected_game() else {
            return;
        };
        let moved = if down {
            self.queue.move_down(&name)
        } else {
            self.queue.move_up(&name)
        };
        if !moved {
            return;
        }
        if self.input.is_empty() {
            self.selected = if down {
                self.selected.saturating_add(1)
            } else {
                self.selected.saturating_sub(1)
            };
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
        eprintln!("No games loaded. Run `backlog setup` to configure, then `backlog sync`.");
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

/// Renders the status-row filter chip, repeating the filter's row glyph so
/// cycling filters teaches the marker vocabulary.
fn filter_chip(filter: crate::queue::Filter) -> String {
    match filter.glyph() {
        Some(glyph) => format!(" {glyph} {} ", filter.label()),
        None => format!(" {} ", filter.label()),
    }
}

/// Background color for a filter's chip, matching its row glyph color.
fn filter_chip_color(filter: crate::queue::Filter) -> Color {
    match filter {
        crate::queue::Filter::Queued => Color::Cyan,
        crate::queue::Filter::Played => Color::Green,
        crate::queue::Filter::All | crate::queue::Filter::Unplayed => Color::Gray,
    }
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

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let title = format!(
        " backlog \u{2500}\u{2500} {} games ({}) ",
        app.games.len(),
        app.sync_age,
    );

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
            Constraint::Length(2), // search input + blank gap
            Constraint::Min(1),    // results
            Constraint::Length(1), // sync status
        ])
        .split(inner);

    // Search input row
    let input_display = format!("> {}", app.input);
    let input_widget =
        Paragraph::new(input_display.as_str()).style(Style::default().fg(Color::White));
    f.render_widget(input_widget, chunks[0]);

    // Place blinking cursor after the prompt characters
    let cursor_x = chunks[0].x + 2 + app.input[..app.cursor_pos].chars().count() as u16;
    let cursor_y = chunks[0].y;
    f.set_cursor_position((cursor_x, cursor_y));

    let hint = match &app.sync_state {
        SyncState::Idle => "enter queue   ^p played   tab filter".to_string(),
        SyncState::Syncing => "syncing...".to_string(),
        SyncState::Done(reports) => format_sync_summary(reports, &app.sync_before_counts),
    };
    let status_line = Line::from(vec![
        Span::styled(
            filter_chip(app.filter),
            Style::default()
                .fg(Color::Black)
                .bg(filter_chip_color(app.filter)),
        ),
        Span::raw("  "),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(status_line), chunks[2]);

    // Only show results when the user has typed something, unless a queue
    // filter is narrowing the list on its own
    if app.input.is_empty() && app.filter == crate::queue::Filter::All {
        return;
    }

    let filtered = app.filtered();
    let results = search::fuzzy_search(&app.input, &filtered);
    if results.is_empty() {
        return;
    }

    // Clamp selected index to valid range
    let max_selected = results.len() - 1;
    let selected = app.selected.min(max_selected);

    let results_area = chunks[1];
    let visible_rows = results_area.height as usize;

    // Scroll offset keeps selected row in view
    let scroll_offset = if selected >= visible_rows {
        selected - visible_rows + 1
    } else {
        0
    };

    // Compute column widths; cap name length to leave room for platform label
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
        let is_selected = abs_idx == selected;
        let is_even = abs_idx % 2 == 0;

        let row_area = Rect::new(results_area.x, row_y, results_area.width, 1);

        // Build styled spans for the game name with match highlighting
        let match_set: std::collections::HashSet<u32> =
            result.match_indices.iter().copied().collect();

        let mut name_spans: Vec<Span> = Vec::new();
        if app.filter == crate::queue::Filter::Queued
            && let Some(rank) = app.queue.rank(&result.game.name)
        {
            name_spans.push(Span::styled(
                format!("{rank:>2} "),
                Style::default().fg(Color::DarkGray),
            ));
        }

        let state = app.queue.state(&result.game.name);
        name_spans.extend(marker_spans(state));
        let base_color = if state.played {
            Color::Gray
        } else {
            Color::White
        };
        name_spans.extend(result.game.name.chars().enumerate().map(|(char_idx, ch)| {
            let style = if match_set.contains(&(char_idx as u32)) {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(base_color)
            };
            Span::styled(ch.to_string(), style)
        }));

        // Pad name to align platform column
        let name_char_count = result.game.name.chars().count();
        let padding = max_name_len.saturating_sub(name_char_count) + 2;
        name_spans.push(Span::raw(" ".repeat(padding)));

        for (i, platform) in result.game.platforms.iter().enumerate() {
            if i > 0 {
                name_spans.push(Span::styled(" / ", Style::default().fg(Color::Gray)));
            }
            name_spans.push(Span::styled(
                platform.as_str(),
                Style::default().fg(platform_color(platform)),
            ));
        }

        let mut row_style = Style::default();
        if is_selected {
            row_style = row_style.bg(Color::DarkGray);
        } else if is_even {
            row_style = row_style.bg(Color::Black);
        }

        let paragraph = Paragraph::new(Line::from(name_spans)).style(row_style);
        f.render_widget(paragraph, row_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{SyncReport, SyncStatus};

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
    fn filter_chip_repeats_the_row_glyph() {
        use crate::queue::Filter;
        assert_eq!(filter_chip(Filter::All), " all ");
        assert_eq!(filter_chip(Filter::Queued), " » queued ");
        assert_eq!(filter_chip(Filter::Played), " ✓ played ");
        assert_eq!(filter_chip(Filter::Unplayed), " unplayed ");
    }
}
