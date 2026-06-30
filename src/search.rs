use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::Game;

/// A game that matched a fuzzy search query, with its score and match positions.
pub struct SearchResult<'a> {
    pub game: &'a Game,
    pub score: u32,
    /// Character positions in `game.name` that matched the query, for TUI highlighting.
    pub match_indices: Vec<u32>,
}

/// Fuzzy-searches `library` using nucleo-matcher, returning matches sorted by score descending.
///
/// Multi-word queries (e.g. "tomb raider") split into atoms that ALL must match.
/// An empty query returns every game with score 0.
pub fn fuzzy_search<'a>(query: &str, library: &'a [Game]) -> Vec<SearchResult<'a>> {
    if library.is_empty() {
        return Vec::new();
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut indices_buf: Vec<u32> = Vec::new();

    let mut results: Vec<SearchResult<'a>> = library
        .iter()
        .filter_map(|game| {
            let haystack = Utf32Str::new(&game.name, &mut buf);
            let score = pattern.score(haystack, &mut matcher)?;
            indices_buf.clear();
            let haystack = Utf32Str::new(&game.name, &mut buf);
            pattern.indices(haystack, &mut matcher, &mut indices_buf);
            Some(SearchResult {
                game,
                score,
                match_indices: indices_buf.clone(),
            })
        })
        .collect();

    results.sort_by(|a, b| b.score.cmp(&a.score));

    if let Some(best) = results.first() {
        let threshold = best.score * 3 / 5;
        results.retain(|r| r.score >= threshold);
    }

    results
}
