//! Regex engines. PatternList tries patterns in order, not earliest-match.

use std::sync::Arc;

/// Leftmost match at or after `offset`; `^`/`$` see the whole page (REG_NOTBOL).
pub trait RegexEngine: Send + Sync {
    /// First match starting at or after `offset`, or `None`.
    fn find_at(&self, text: &str, offset: usize) -> Option<(usize, usize)>;
}

pub type SharedEngine = Arc<dyn RegexEngine>;

/// Default engine: `regex` crate (leftmost-first, `.` skips `\n`).
pub struct RegexEngineImpl {
    re: regex::Regex,
}

impl RegexEngineImpl {
    pub fn new(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { re })
    }
}

impl RegexEngine for RegexEngineImpl {
    fn find_at(&self, text: &str, offset: usize) -> Option<(usize, usize)> {
        self.re.find_at(text, offset).map(|m| (m.start(), m.end()))
    }
}

/// `-P` engine: fancy-regex (look-around, backreferences).
pub struct PcreEngine {
    re: fancy_regex::Regex,
}

impl PcreEngine {
    pub fn new(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        let re = fancy_regex::RegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { re })
    }
}

impl RegexEngine for PcreEngine {
    fn find_at(&self, text: &str, offset: usize) -> Option<(usize, usize)> {
        match self.re.find_from_pos(text, offset) {
            Ok(Some(m)) => Some((m.start(), m.end())),
            // Backtrack/runtime error: treat as no match (like PCRE match-limit).
            Ok(None) | Err(_) => None,
        }
    }
}

/// `-F` engine; newline-separated needles, leftmost then first-listed wins.
pub struct FixedEngine {
    re: regex::Regex,
}

impl FixedEngine {
    pub fn new(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        let needles = split_fixed_needles(pattern);
        let alternation = needles
            .iter()
            .map(|n| regex::escape(n))
            .collect::<Vec<_>>()
            .join("|");
        // An empty needle list cannot happen (split always yields >= 1 item).
        let re = regex::RegexBuilder::new(&alternation)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { re })
    }
}

/// Split like C getline: newlines separate, trailing newline dropped.
fn split_fixed_needles(pattern: &str) -> Vec<&str> {
    let mut needles: Vec<&str> = pattern.split('\n').collect();
    if pattern.ends_with('\n') {
        needles.pop();
    }
    if needles.is_empty() {
        needles.push("");
    }
    needles
}

impl RegexEngine for FixedEngine {
    fn find_at(&self, text: &str, offset: usize) -> Option<(usize, usize)> {
        self.re.find_at(text, offset).map(|m| (m.start(), m.end()))
    }
}

/// Never matches; used for an empty `-e`/`-f` pattern list.
pub struct NeverEngine;

impl RegexEngine for NeverEngine {
    fn find_at(&self, _text: &str, _offset: usize) -> Option<(usize, usize)> {
        None
    }
}

/// First pattern that matches from the offset wins, not the earliest span.
pub struct PatternList {
    engines: Vec<SharedEngine>,
}

impl PatternList {
    pub fn new(engines: Vec<SharedEngine>) -> Self {
        Self { engines }
    }
}

impl RegexEngine for PatternList {
    fn find_at(&self, text: &str, offset: usize) -> Option<(usize, usize)> {
        for e in &self.engines {
            if let Some(m) = e.find_at(text, offset) {
                return Some(m);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(engine: &dyn RegexEngine, text: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut idx = 0;
        while let Some((s, e)) = engine.find_at(text, idx) {
            out.push((s, e));
            idx = e;
            if s == e {
                idx += 1;
            }
            if idx >= text.len() {
                break;
            }
        }
        out
    }

    #[test]
    fn caret_only_matches_page_start() {
        let re = RegexEngineImpl::new("^foo", false).unwrap();
        assert_eq!(re.find_at("xfoo foo", 0), None);
        assert_eq!(re.find_at("foo foo", 0), Some((0, 3)));
        // Second call from offset 3: REG_NOTBOL equivalent, no match.
        assert_eq!(re.find_at("foo foo", 3), None);
    }

    #[test]
    fn dollar_matches_page_end_only() {
        let re = RegexEngineImpl::new("foo$", false).unwrap();
        // $ is end of page text, not end of line.
        assert_eq!(re.find_at("foo bar\nfoofoo\nfoo", 0), Some((15, 18)));
    }

    #[test]
    fn empty_pattern_matches_every_position_once_per_line_group() {
        let re = RegexEngineImpl::new("", false).unwrap();
        assert_eq!(all(&re, "ab"), vec![(0, 0), (1, 1)]);
        assert_eq!(all(&re, ""), vec![(0, 0)]);
    }

    #[test]
    fn fixed_needle_split_matches_getline_semantics() {
        assert_eq!(split_fixed_needles("a\nb"), vec!["a", "b"]);
        assert_eq!(split_fixed_needles("a\n"), vec!["a"]);
        assert_eq!(split_fixed_needles(""), vec![""]);
        assert_eq!(split_fixed_needles("\n"), vec![""]);
        assert_eq!(split_fixed_needles("a\n\nb"), vec!["a", "", "b"]);
    }

    #[test]
    fn fixed_engine_leftmost_wins() {
        let re = FixedEngine::new("two\none", false).unwrap();
        assert_eq!(re.find_at("one two", 0), Some((0, 3)));
        assert_eq!(re.find_at("one two", 4), Some((4, 7)));
    }

    #[test]
    fn pattern_list_first_pattern_wins_not_earliest() {
        // First pattern wins even if a later pattern would match earlier.
        let a = Arc::new(RegexEngineImpl::new("late", false).unwrap()) as SharedEngine;
        let b = Arc::new(RegexEngineImpl::new("early", false).unwrap()) as SharedEngine;
        let list = PatternList::new(vec![a, b]);
        // "late" starts at byte 10 of "early ... late"
        assert_eq!(list.find_at("early ... late", 0), Some((10, 14)));
    }

    #[test]
    fn ignore_case() {
        let re = RegexEngineImpl::new("foobar", true).unwrap();
        assert_eq!(re.find_at("FoObAr", 0), Some((0, 6)));
        let f = FixedEngine::new("foobar", true).unwrap();
        assert_eq!(f.find_at("FoObAr", 0), Some((0, 6)));
    }
}
