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
///
/// Strong hits come first and, when there are any, are the only results: titles
/// that contain every word literally (ignoring case and accents), and, for a
/// single-word query of two or more characters, titles whose word initials
/// spell it (`gta` for "Grand Theft Auto V"). Titles whose initials START with
/// the query rank above all other strong hits, so `tr` puts "Tomb Raider" ahead
/// of its many substring hits. This keeps scattered subsequence matches like
/// `kerbal` against "Kizuna Encounter: Super Tag Battle" from crowding out real
/// hits. Only when there are no strong hits does it fall back to fuzzy matching,
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
    let acronym = match words.as_slice() {
        [word] if word.chars().count() >= 2 => Some(word.as_str()),
        _ => None,
    };
    let acronym_hits: Vec<Option<(usize, Vec<usize>)>> = names
        .iter()
        .map(|name| acronym.and_then(|query| match_initials(query, &name.text)))
        .collect();
    let literal: Vec<bool> = names
        .iter()
        .map(|name| words.iter().all(|w| name.text.contains(w.as_str())))
        .collect();
    let strong: Vec<usize> = (0..library.len())
        .filter(|&i| literal[i] || acronym_hits[i].is_some())
        .collect();

    let mut initial_prefix_hits = Vec::new();
    let mut other_strong_hits = Vec::new();
    for (i, mut result) in score_all(&words, &strong, &names, library, false) {
        match &acronym_hits[i] {
            Some((start, positions)) if *start == 0 || !literal[i] => {
                result.match_indices = names[i].original_indices(positions);
                if *start == 0 {
                    initial_prefix_hits.push(result);
                } else {
                    other_strong_hits.push(result);
                }
            }
            _ => other_strong_hits.push(result),
        }
    }
    if !initial_prefix_hits.is_empty() || !other_strong_hits.is_empty() {
        // Scores must stay descending across the whole list, so lift the prefix
        // group just above the best of the rest.
        if let (Some(lowest), Some(best_other)) =
            (initial_prefix_hits.last(), other_strong_hits.first())
            && lowest.score <= best_other.score
        {
            let lift = best_other.score - lowest.score + 1;
            initial_prefix_hits.iter_mut().for_each(|r| r.score += lift);
        }
        initial_prefix_hits.append(&mut other_strong_hits);
        return initial_prefix_hits;
    }

    let everything: Vec<usize> = (0..library.len()).collect();
    let mut results: Vec<SearchResult> = score_all(&words, &everything, &names, library, true)
        .into_iter()
        .map(|(_, result)| result)
        .collect();
    if let Some(best) = results.first() {
        let threshold = best.score * 4 / 5;
        results.retain(|r| r.score >= threshold);
    }
    results
}

impl Folded {
    /// Maps byte offsets in `text` to sorted, deduplicated char positions in the
    /// original name.
    fn original_indices(&self, bytes: &[usize]) -> Vec<u32> {
        let mut indices: Vec<u32> = bytes
            .iter()
            .filter_map(|&byte| self.origin.get(byte).copied())
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    }
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

/// Byte offsets in `folded` where a word starts: an alphanumeric char that
/// follows a non-alphanumeric one. An apostrophe inside a word ("Death's") does
/// not start a new one.
fn initials(folded: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let (mut prev, mut before_prev) = (None, None);
    for (byte, c) in folded.char_indices() {
        let starts_word = c.is_alphanumeric()
            && match prev {
                None => true,
                Some('\'' | '’') => !before_prev.is_some_and(char::is_alphanumeric),
                Some(p) => !p.is_alphanumeric(),
            };
        if starts_word {
            starts.push(byte);
        }
        (before_prev, prev) = (prev, Some(c));
    }
    starts
}

/// Finds `query` as consecutive word initials of `folded`, returning which
/// initial it starts at and the byte offsets of the matched initials.
fn match_initials(query: &str, folded: &str) -> Option<(usize, Vec<usize>)> {
    let query: Vec<char> = query.chars().collect();
    let starts = initials(folded);
    let letters: Vec<char> = starts
        .iter()
        .filter_map(|&byte| folded[byte..].chars().next())
        .collect();
    let at = letters.windows(query.len()).position(|w| w == query)?;
    Some((at, starts[at..at + query.len()].to_vec()))
}

/// Fuzzy-scores the entries at `candidates` against `words`, sorted by score
/// descending, each paired with its index in `library`.
fn score_all<'a>(
    words: &[String],
    candidates: &[usize],
    names: &[Folded],
    library: &'a [LibraryEntry],
    allow_typos: bool,
) -> Vec<(usize, SearchResult<'a>)> {
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
    let mut results: Vec<(usize, SearchResult<'a>)> = Matcher::from_patterns(&patterns, &config)
        .match_list_indices(&haystacks)
        .into_iter()
        .map(|m| {
            let entry = candidates[m.index as usize];
            let name = &names[entry];
            let bytes: Vec<usize> = m.indices.iter().map(|&byte| byte as usize).collect();
            let result = SearchResult {
                game: &library[entry],
                score: u32::from(m.score),
                match_indices: name.original_indices(&bytes),
            };
            (entry, result)
        })
        .collect();
    results.sort_by_key(|(_, r)| std::cmp::Reverse(r.score));
    results
}
