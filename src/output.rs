use crate::itad::{GamePrice, MatchKind, Money, PriceGroup};
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
///
/// Ranks come from the queue itself, not from row position, so a queued game
/// missing from the library leaves a visible gap here exactly as it does in the
/// TUI rather than being silently renumbered away.
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
        .map(|e| {
            let rank = queue
                .rank(&e.name)
                .map_or(String::new(), |rank| rank.to_string());
            let played = if queue.state(&e.name).played {
                PLAYED_GLYPH
            } else {
                ' '
            };
            let platforms = e.platforms.join(" / ");
            let pad = max_name_len - e.name.chars().count();
            format!(
                "{rank:>2} {played} {}{}  {platforms}",
                e.name,
                " ".repeat(pad)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Formats a price as `$20.99` for USD, otherwise `20.99 EUR`.
pub fn format_money(m: &Money) -> String {
    if m.currency == "USD" {
        format!("${:.2}", m.amount)
    } else {
        format!("{:.2} {}", m.amount, m.currency)
    }
}

fn with_cut(price: &Money, cut: u32) -> String {
    let price = format_money(price);
    if cut == 0 {
        price
    } else {
        format!("{price} -{cut}%")
    }
}

fn price_cells(g: &GamePrice) -> [String; 3] {
    let dash = || "-".to_string();
    [
        g.steam
            .as_ref()
            .map_or_else(dash, |s| with_cut(&s.price, s.cut)),
        g.best.as_ref().map_or_else(dash, |b| {
            format!("{} {}", with_cut(&b.price, b.cut), b.shop)
        }),
        g.low.as_ref().map_or_else(dash, |l| {
            format!("{} {} {}", format_money(&l.price), l.shop, l.date)
        }),
    ]
}

/// Formats price results as one aligned table: exact matches first, then a
/// block of candidates for each input that matched inexactly. Inputs with no
/// match are omitted; the caller reports them.
pub fn format_price_groups(groups: &[PriceGroup]) -> String {
    let row = |title: String, kind: &str, g: &GamePrice| {
        let [steam, best, low] = price_cells(g);
        [title, kind.to_string(), steam, best, low]
    };
    let exact: Vec<[String; 5]> = groups
        .iter()
        .filter(|g| g.kind == MatchKind::Exact)
        .flat_map(|g| &g.games)
        .map(|g| row(g.title.clone(), "", g))
        .collect();
    let candidates: Vec<(&str, Vec<[String; 5]>)> = groups
        .iter()
        .filter(|g| g.kind == MatchKind::Candidates)
        .map(|g| {
            let rows = g
                .games
                .iter()
                .map(|c| row(format!("  {}", c.title), &c.kind, c))
                .collect();
            (g.input.as_str(), rows)
        })
        .collect();
    if exact.is_empty() && candidates.is_empty() {
        return String::new();
    }

    let header = ["Title", "", "Steam", "Best now", "All-time low"].map(String::from);
    let all_rows = || {
        std::iter::once(&header)
            .chain(&exact)
            .chain(candidates.iter().flat_map(|(_, rows)| rows))
    };
    let widths: [usize; 4] =
        std::array::from_fn(|i| all_rows().map(|r| r[i].chars().count()).max().unwrap_or(0));
    let render = |r: &[String; 5]| {
        let kind = if widths[1] > 0 {
            format!("{:<w$}  ", r[1], w = widths[1])
        } else {
            String::new()
        };
        let line = format!(
            "{:<w0$}  {kind}{:<w2$}  {:<w3$}  {}",
            r[0],
            r[2],
            r[3],
            r[4],
            w0 = widths[0],
            w2 = widths[2],
            w3 = widths[3]
        );
        line.trim_end().to_string()
    };

    let mut lines = vec![render(&header)];
    lines.extend(exact.iter().map(render));
    for (input, rows) in &candidates {
        lines.push(String::new());
        lines.push(format!("{input}: no exact match, candidates:"));
        lines.extend(rows.iter().map(render));
    }
    lines.join("\n")
}
