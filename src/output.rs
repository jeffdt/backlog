use crate::cart::{self, CartRow, ItemKind, Verdict};
use crate::itad::{GamePrice, LowPrice, MatchKind, Money, PriceGroup};
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

fn column_widths<'a, const N: usize>(
    rows: impl IntoIterator<Item = &'a [String; N]>,
) -> [usize; N] {
    let mut widths = [0; N];
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    widths
}

/// Pads every cell but the last to its column width, two spaces apart,
/// dropping columns that are empty in every row.
fn render_aligned<const N: usize>(row: &[String; N], widths: &[usize; N]) -> String {
    let cells: Vec<String> = row
        .iter()
        .zip(widths)
        .enumerate()
        .filter(|(i, (_, w))| **w > 0 || i + 1 == N)
        .map(|(i, (cell, w))| {
            if i + 1 == N {
                cell.clone()
            } else {
                format!("{cell:<w$}")
            }
        })
        .collect();
    cells.join("  ").trim_end().to_string()
}

fn low_cell(l: &LowPrice) -> String {
    format!("{} {} {}", format_money(&l.price), l.shop, l.date)
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
        g.low.as_ref().map_or_else(dash, low_cell),
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
    let widths = column_widths(all_rows());
    let render = |r: &[String; 5]| render_aligned(r, &widths);

    let mut lines = vec![render(&header)];
    lines.extend(exact.iter().map(render));
    for (input, rows) in &candidates {
        lines.push(String::new());
        lines.push(format!("{input}: no exact match, candidates:"));
        lines.extend(rows.iter().map(render));
    }
    lines.join("\n")
}

fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::AtLow => "at low",
        Verdict::AboveLow => "above low",
        Verdict::Owned => "already owned",
        Verdict::NothingNew => "nothing new",
        Verdict::Unknown => "no low known",
    }
}

/// Formats judged cart items as an aligned table followed by cart totals.
///
/// Bundles, and packages holding more than one game, list their games
/// underneath with each game's low or why it does not count.
pub fn format_cart_rows(rows: &[CartRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut table = vec![["Item", "Cart", "All-time low", ""].map(String::from)];
    for row in rows {
        let receiving: Vec<_> = row.apps.iter().filter(|a| !a.excluded).collect();
        let low = match (receiving.as_slice(), &row.low_total) {
            ([only], Some(_)) => only.low.as_ref().map_or_else(String::new, low_cell),
            (_, Some(total)) => format_money(total),
            (_, None) => "-".to_string(),
        };
        let name = match row.kind {
            ItemKind::Bundle => format!("{} (bundle)", row.name),
            ItemKind::Package => row.name.clone(),
        };
        let mut verdict = verdict_label(row.verdict).to_string();
        if row.verdict == Verdict::Owned {
            let mut platforms: Vec<&str> = receiving
                .iter()
                .flat_map(|a| &a.owned_on)
                .map(String::as_str)
                .collect();
            platforms.sort_unstable();
            platforms.dedup();
            verdict = format!("{verdict} ({})", platforms.join(" / "));
        }
        table.push([name, format_money(&row.price), low, verdict]);
        if row.kind == ItemKind::Package && row.apps.len() <= 1 {
            continue;
        }
        for app in &row.apps {
            let note = if app.excluded {
                "owned on steam, excluded".to_string()
            } else if app.owned_on.is_empty() {
                String::new()
            } else {
                format!("owned on {}", app.owned_on.join(" / "))
            };
            let low = if app.excluded {
                String::new()
            } else {
                app.low.as_ref().map_or_else(|| "-".to_string(), low_cell)
            };
            table.push([format!("  {}", app.name), String::new(), low, note]);
        }
    }
    let widths = column_widths(&table);
    let mut lines: Vec<String> = table.iter().map(|r| render_aligned(r, &widths)).collect();
    lines.push(String::new());
    lines.extend(format_cart_totals(&cart::totals(rows)));
    lines.join("\n")
}

fn format_cart_totals(t: &cart::CartTotals) -> Vec<String> {
    let left_out = |n: usize| match n {
        0 => String::new(),
        1 => "  (1 item with no price left out)".to_string(),
        n => format!("  ({n} items with no price left out)"),
    };
    let rows = [
        ("Cart total", format_money(&t.cart), String::new()),
        (
            "At best prices now",
            format_money(&t.best_now),
            left_out(t.best_now_missing),
        ),
        (
            "At all-time lows",
            format_money(&t.low),
            left_out(t.low_missing),
        ),
    ];
    let width = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
    rows.iter()
        .map(|(label, amount, note)| format!("{label:<18}  {amount:>width$}{note}"))
        .collect()
}
