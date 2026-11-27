use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str, chars};

use crate::LibraryEntry;

/// A library entry that matched a fuzzy search query, with its score and match positions.
pub struct SearchResult<'a> {
    pub game: &'a LibraryEntry,
    pub score: u32,
    /// Character positions in `game.name` that matched the query, for TUI highlighting.
    pub match_indices: Vec<u32>,
}

/// Searches `library` for `query`, returning matches sorted by score descending.
///
/// Multi-word queries (e.g. "tomb raider") split into words that ALL must match.
/// When any title contains every word literally (ignoring case and accents), only those
/// titles are returned, so scattered subsequence matches like `kerbal` against
/// "Kizuna Encounter: Super Tag Battle" never crowd out real hits. Only when no
/// title matches literally does it fall back to fuzzy matching, which keeps
/// typos and abbreviations working; weak fuzzy matches below `best * 3/5` are
/// pruned. A blank query returns every entry with score 0.
pub fn fuzzy_search<'a>(query: &str, library: &'a [LibraryEntry]) -> Vec<SearchResult<'a>> {
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT);

    // Checked by hand rather than with AtomKind::Substring: in nucleo-matcher
    // 0.3.1 that atom misses a match ending on the last char of any non-ASCII
    // name ("knight" vs "Batman™: Arkham Knight").
    let words: Vec<String> = query.split_whitespace().map(fold).collect();
    let literal = library.iter().filter(|entry| {
        let name = fold(&entry.name);
        words.iter().all(|word| name.contains(word.as_str()))
    });
    let literal = score_all(&pattern, literal, &mut matcher);
    if !literal.is_empty() {
        return literal;
    }

    let mut results = score_all(&pattern, library.iter(), &mut matcher);
    if let Some(best) = results.first() {
        let threshold = best.score * 3 / 5;
        results.retain(|r| r.score >= threshold);
    }
    results
}

/// Lowercases `text` and strips diacritics the same way the matcher does, one
/// char per grapheme, so a literal check agrees with what the matcher scores.
fn fold(text: &str) -> String {
    chars::graphemes(text)
        .map(|c| chars::to_lower_case(chars::normalize(c)))
        .collect()
}

/// Scores `entries` against `pattern`, sorted by score descending.
fn score_all<'a>(
    pattern: &Pattern,
    entries: impl Iterator<Item = &'a LibraryEntry>,
    matcher: &mut Matcher,
) -> Vec<SearchResult<'a>> {
    let mut buf = Vec::new();
    let mut results: Vec<SearchResult<'a>> = entries
        .filter_map(|entry| {
            let mut match_indices = Vec::new();
            let score = pattern.indices(
                Utf32Str::new(&entry.name, &mut buf),
                matcher,
                &mut match_indices,
            )?;
            Some(SearchResult {
                game: entry,
                score,
                match_indices,
            })
        })
        .collect();
    results.sort_by_key(|r| std::cmp::Reverse(r.score));
    results
}
