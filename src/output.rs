use crate::queue::{PLAYED_GLYPH, QUEUED_GLYPH, Queue};
use crate::search::SearchResult;
use crate::{LibraryEntry, queue};

/// Renders the two-character queue marker field for a game.
///
/// Uncolored, so piped CLI output stays clean.
pub fn marker_field(state: queue::GameState) -> String {
    let queued = if state.queued { QUEUED_GLYPH } else { ' ' };
    let played = if state.played { PLAYED_GLYPH } else { ' ' };
    format!("{queued}{played}")
}

/// Formats one-shot search results as aligned rows prefixed with queue markers.
pub fn format_search_rows(results: &[SearchResult<'_>], queue: &Queue) -> String {
    if results.is_empty() {
        return String::new();
    }
    let max_name_len = results
        .iter()
        .map(|r| r.game.name.chars().count())
        .max()
        .unwrap_or(0);
    results
        .iter()
        .map(|r| {
            let markers = marker_field(queue.state(&r.game.name));
            let platforms = r.game.platforms.join(" / ");
            let pad = max_name_len - r.game.name.chars().count();
            format!("{markers} {}{}  {platforms}", r.game.name, " ".repeat(pad))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Formats the queue as ranked rows, skipping games no longer in the library.
pub fn format_queue_rows(library: &[LibraryEntry], queue: &Queue) -> String {
    let entries = queue::apply_filter(library, queue, queue::Filter::Queued);
    if entries.is_empty() {
        return String::new();
    }
    let max_name_len = entries
        .iter()
        .map(|e| e.name.chars().count())
        .max()
        .unwrap_or(0);
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let played = if queue.state(&e.name).played {
                PLAYED_GLYPH
            } else {
                ' '
            };
            let platforms = e.platforms.join(" / ");
            let pad = max_name_len - e.name.chars().count();
            format!(
                "{:>2} {played} {}{}  {platforms}",
                i + 1,
                e.name,
                " ".repeat(pad)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
