use frizbee::{CaseMatching, Config, Matcher, Pattern, PatternConfig, UnicodeMatching};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::LibraryEntry;

/// A library entry that matched a fuzzy search query, with its score and match positions.
pub struct SearchResult<'a> {
    pub game: &'a LibraryEntry,
    pub score: u32,
    /// Character positions in `game.name` that matched the query, for TUI highlighting.
    pub match_indices: Vec<u32>,
}

/// A name lowercased with diacritics stripped.
struct Folded {
    text: String,
    /// For each byte of `text`, the index of the original char it came from, since
    /// frizbee reports match positions as byte offsets into the text it was given.
    origin: Vec<u32>,
}

/// Searches `library` for `query`, returning matches sorted by score descending.
///
/// Multi-word queries (e.g. "tomb raider") split into words that ALL must match.
/// When any title contains every word literally (ignoring case and accents), only
/// those titles are returned, so scattered subsequence matches like `kerbal`
/// against "Kizuna Encounter: Super Tag Battle" never crowd out literal hits.
/// Only when no title matches literally does it fall back to fuzzy matching,
/// which tolerates one typo per four letters of each word and keeps
/// abbreviations working; weak fuzzy matches below `best * 4/5` are pruned.
/// A blank query returns every entry with score 0.
pub fn fuzzy_search<'a>(query: &str, library: &'a [LibraryEntry]) -> Vec<SearchResult<'a>> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| fold(w).text)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return library
            .iter()
            .map(|game| SearchResult {
                game,
                score: 0,
                match_indices: Vec::new(),
            })
            .collect();
    }

    let names: Vec<Folded> = library.iter().map(|e| fold(&e.name)).collect();
    let literal: Vec<usize> = (0..library.len())
        .filter(|&i| words.iter().all(|w| names[i].text.contains(w.as_str())))
        .collect();
    let literal = score_all(&words, &literal, &names, library, false);
    if !literal.is_empty() {
        return literal;
    }

    let everything: Vec<usize> = (0..library.len()).collect();
    let mut results = score_all(&words, &everything, &names, library, true);
    if let Some(best) = results.first() {
        let threshold = best.score * 4 / 5;
        results.retain(|r| r.score >= threshold);
    }
    results
}

/// Lowercases `text` and strips diacritics, recording where each kept byte came from.
fn fold(text: &str) -> Folded {
    let mut folded = Folded {
        text: String::new(),
        origin: Vec::new(),
    };
    for (i, c) in text.chars().enumerate() {
        for lower in c.to_lowercase() {
            for base in lower.nfd().filter(|d| !is_combining_mark(*d)) {
                let start = folded.text.len();
                match ascii_spelling(base) {
                    Some(spelling) => folded.text.push_str(spelling),
                    None => folded.text.push(base),
                }
                let added = folded.text.len() - start;
                folded.origin.extend(std::iter::repeat_n(i as u32, added));
            }
        }
    }
    folded
}

/// ASCII spelling for lowercase letters that have no Unicode decomposition to strip.
fn ascii_spelling(c: char) -> Option<&'static str> {
    Some(match c {
        'ł' => "l",
        'ø' => "o",
        'đ' => "d",
        'ı' => "i",
        'ħ' => "h",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'þ' => "th",
        _ => return None,
    })
}

/// Fuzzy-scores the entries at `candidates` against `words`, sorted by score descending.
fn score_all<'a>(
    words: &[String],
    candidates: &[usize],
    names: &[Folded],
    library: &'a [LibraryEntry],
    allow_typos: bool,
) -> Vec<SearchResult<'a>> {
    let patterns: Vec<Pattern> = words
        .iter()
        .map(|w| {
            let typos = if allow_typos {
                (w.chars().count() / 4) as u16
            } else {
                0
            };
            Pattern::new(w, PatternConfig::default()).max_typos(Some(typos))
        })
        .collect();
    let config = Config {
        casing: CaseMatching::Ignore,
        unicode: UnicodeMatching::Always,
        ..Config::default()
    };
    let haystacks: Vec<&str> = candidates.iter().map(|&i| names[i].text.as_str()).collect();
    let mut results: Vec<SearchResult<'a>> = Matcher::from_patterns(&patterns, &config)
        .match_list_indices(&haystacks)
        .into_iter()
        .map(|m| {
            let entry = candidates[m.index as usize];
            let name = &names[entry];
            let mut match_indices: Vec<u32> = m
                .indices
                .iter()
                .filter_map(|&byte| name.origin.get(byte as usize).copied())
                .collect();
            match_indices.sort_unstable();
            match_indices.dedup();
            SearchResult {
                game: &library[entry],
                score: u32::from(m.score),
                match_indices,
            }
        })
        .collect();
    results.sort_by_key(|r| std::cmp::Reverse(r.score));
    results
}
