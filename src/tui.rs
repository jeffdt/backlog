use std::io;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::LibraryEntry;
use crate::loader::{self, LoadResult};
use crate::search;
use crate::sync::{SyncReport, SyncStatus};

struct App {
    input: String,
    cursor_pos: usize,
    games: Vec<LibraryEntry>,
    selected: usize,
    sync_age: String,
    quit: bool,
}

impl App {
    fn new(load_result: LoadResult) -> Self {
        let sync_age = loader::format_sync_age(load_result.oldest_update);
        Self {
            input: String::new(),
            cursor_pos: 0,
            games: load_result.games,
            selected: 0,
            sync_age,
            quit: false,
        }
    }
}

/// Launches the interactive TUI, loading games from `cache_dir`.
pub fn run(cache_dir: &Path) -> io::Result<()> {
    let load_result = loader::load_all_games(cache_dir);
    run_with(load_result)
}

/// Launches the interactive TUI with a pre-loaded game list.
pub fn run_with(load_result: LoadResult) -> io::Result<()> {
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

    let mut app = App::new(load_result);
    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

const TICK_RATE: Duration = Duration::from_millis(100);

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        if app.quit {
            return Ok(());
        }

        if event::poll(TICK_RATE)? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        app.quit = true;
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
                SyncStatus::Ok => format!("+{}", r.game_count),
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

    // Only show results when the user has typed something
    if app.input.is_empty() {
        return;
    }

    let results = search::fuzzy_search(&app.input, &app.games);
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
        .min(available_width.saturating_sub(10));

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

        let mut name_spans: Vec<Span> = result
            .game
            .name
            .chars()
            .enumerate()
            .map(|(char_idx, ch)| {
                let style = if match_set.contains(&(char_idx as u32)) {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                Span::styled(ch.to_string(), style)
            })
            .collect();

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
            row_style = row_style.bg(Color::Rgb(30, 30, 30));
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
    fn all_ok_reports_show_plus_counts() {
        let reports = vec![ok("steam", 42), ok("epic", 10)];
        assert_eq!(format_sync_summary(&reports), "steam +42  epic +10");
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
        assert_eq!(
            format_sync_summary(&reports),
            "steam +42  gog skip  amazon error"
        );
    }

    #[test]
    fn empty_reports_produce_empty_string() {
        assert_eq!(format_sync_summary(&[]), "");
    }
}
