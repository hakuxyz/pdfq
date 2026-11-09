//! `--page-range`: `N` or `N-M` lists; empty means all pages; `2-1` = none.

use std::fmt;

#[derive(Debug, thiserror::Error)]
#[error("Invalid page range \"{input}\". {reason}")]
pub struct PageRangeError {
    input: String,
    reason: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct Interval {
    from: u32,
    to: u32,
}

impl Interval {
    fn contains(&self, page: u32) -> bool {
        page >= self.from && page <= self.to
    }
}

#[derive(Debug, Clone, Default)]
pub struct PageRange {
    intervals: Vec<Interval>,
}

impl PageRange {
    /// Parse range; Default matches all, a provided-but-empty range matches none.
    pub fn parse(input: &str) -> Result<Self, PageRangeError> {
        let mut intervals = Vec::new();
        for part in input.split(',') {
            intervals.push(parse_interval(part)?);
        }
        Ok(Self { intervals })
    }

    pub fn contains(&self, page: u32) -> bool {
        // No ranges at all: search every page.
        if self.intervals.is_empty() {
            return true;
        }
        self.intervals.iter().any(|i| i.contains(page))
    }

    pub fn is_explicitly_empty(&self) -> bool {
        !self.intervals.is_empty() && self.intervals.iter().all(|i| i.to < i.from)
    }
}

fn parse_interval(s: &str) -> Result<Interval, PageRangeError> {
    let err = |reason: &'static str| PageRangeError {
        input: s.to_string(),
        reason,
    };

    let (from_s, to_s) = match s.split_once('-') {
        Some((a, b)) => (a, b),
        None => (s, s),
    };

    let parse_num = |v: &str| -> Result<u32, PageRangeError> {
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err(
                "Expected a single page or a range PAGE1-PAGE2.",
            ));
        }
        v.parse::<u32>()
            .map_err(|_| err("Expected a single page or a range PAGE1-PAGE2."))
    };

    let from = parse_num(from_s)?;
    let to = parse_num(to_s)?;
    if from == 0 || to == 0 {
        return Err(err("Page numbers must be positive."));
    }
    Ok(Interval { from, to })
}

/// One-line warning for inverted ranges, matching pdfgrep's wording.
pub fn empty_range_warning(range: &PageRange, input: &str) -> Option<String> {
    if range.is_explicitly_empty() {
        Some(format!("warning: Page range is empty: {input}"))
    } else {
        None
    }
}

impl fmt::Display for PageRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.intervals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_ranges() {
        let r = PageRange::parse("1-2,3").unwrap();
        assert!(r.contains(1) && r.contains(2) && r.contains(3));
        assert!(!r.contains(4));
    }

    #[test]
    fn default_matches_everything() {
        assert!(PageRange::default().contains(1));
        assert!(PageRange::default().contains(999));
    }

    #[test]
    fn inverted_range_matches_nothing() {
        let r = PageRange::parse("2-1").unwrap();
        assert!(r.is_explicitly_empty());
        assert!(!r.contains(1) && !r.contains(2));
    }

    #[test]
    fn rejects_garbage() {
        assert!(PageRange::parse("foo").is_err());
        assert!(PageRange::parse("0--2").is_err());
        assert!(PageRange::parse("1-foo").is_err());
        assert!(PageRange::parse("").is_err());
    }
}
