//! PDF open/decrypt/extract via pdf-extract; strips its leading newlines to match poppler.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use lopdf::{Document, LoadOptions};

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("Could not open {path}: {reason}")]
    Open { path: String, reason: String },
    #[error("Could not open {path}")]
    Encrypted { path: String },
}

/// One page of searchable text plus its label.
pub struct PageText {
    pub text: String,
    pub label: String,
}

pub struct LoadedDoc {
    pages: Vec<PageText>,
    /// 1-based pages that failed extraction (reported, then skipped).
    failed_pages: Vec<u32>,
}

impl LoadedDoc {
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn page(&self, idx: usize) -> Option<&PageText> {
        self.pages.get(idx)
    }

    pub fn failed_pages(&self) -> &[u32] {
        &self.failed_pages
    }

    pub fn from_pages(pages: Vec<PageText>) -> Self {
        Self {
            pages,
            failed_pages: Vec::new(),
        }
    }

    pub fn pages_for_cache(&self) -> Vec<PageText> {
        self.pages
            .iter()
            .map(|p| PageText {
                text: p.text.clone(),
                label: p.label.clone(),
            })
            .collect()
    }
}

/// Open and decrypt with the first working password; password is required at load time (`LoadOptions::with_password`).
pub fn open(bytes: &[u8], path: &str, passwords: &[String]) -> Result<Document, PdfError> {
    let attempts: Vec<&str> = if passwords.is_empty() {
        vec![""]
    } else {
        passwords.iter().map(String::as_str).collect()
    };

    let mut saw_password_error = false;
    for pw in attempts {
        match Document::load_mem_with_options(bytes, LoadOptions::with_password(pw)) {
            Ok(doc) => return Ok(doc),
            Err(lopdf::Error::InvalidPassword) => {
                saw_password_error = true;
            }
            Err(e) => {
                return Err(PdfError::Open {
                    path: path.to_string(),
                    reason: e.to_string(),
                });
            }
        }
    }

    if saw_password_error {
        Err(PdfError::Encrypted {
            path: path.to_string(),
        })
    } else {
        Err(PdfError::Open {
            path: path.to_string(),
            reason: "could not load PDF".to_string(),
        })
    }
}

/// Extract searchable text for every page of an already-opened document.
pub fn extract(doc: &Document) -> LoadedDoc {
    let labels = page_labels(doc);
    let page_ids: Vec<u32> = {
        let map = doc.get_pages();
        let mut ids: Vec<u32> = map.keys().copied().collect();
        ids.sort_unstable();
        ids
    };

    let mut pages = Vec::with_capacity(page_ids.len());
    let mut failed_pages = Vec::new();
    for (i, &pid) in page_ids.iter().enumerate() {
        let text = match extract_page(doc, pid) {
            Some(t) => t,
            None => {
                failed_pages.push((i + 1) as u32);
                String::new()
            }
        };
        pages.push(PageText {
            text,
            label: labels
                .as_ref()
                .and_then(|l| l.get(i).cloned())
                .unwrap_or_else(|| (i + 1).to_string()),
        });
    }

    LoadedDoc {
        pages,
        failed_pages,
    }
}

/// Extract one page; panics from pdf-extract are caught (corrupt page -> None).
fn extract_page(doc: &Document, page_num: u32) -> Option<String> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut s = String::new();
        {
            let mut out = pdf_extract::PlainTextOutput::new(&mut s);
            pdf_extract::output_doc_page(doc, &mut out, page_num).ok()?;
        }
        Some(s)
    }));
    match result {
        Ok(Some(raw)) => Some(normalize_page_text(&raw)),
        Ok(None) => None,
        Err(_) => None,
    }
}

/// Strip pdf-extract's leading newlines and trailing whitespace.
fn normalize_page_text(raw: &str) -> String {
    let start = raw
        .find(|c: char| c != '\n' && c != '\r')
        .unwrap_or(raw.len());
    let trimmed_end = raw[start..].trim_end();
    trimmed_end.to_string()
}

/// Page labels from `/PageLabels`, else decimal numbers (like poppler).
fn page_labels(doc: &Document) -> Option<Vec<String>> {
    let root_id = doc.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let catalog = doc.get_dictionary(root_id).ok()?;
    let labels_obj = catalog.get(b"PageLabels").ok()?;
    let labels_dict = match labels_obj {
        lopdf::Object::Reference(id) => doc.get_dictionary(*id).ok()?,
        lopdf::Object::Dictionary(d) => d,
        _ => return None,
    };
    let nums = labels_dict.get(b"Nums").ok()?.as_array().ok()?;

    // [(start_page_idx, dict), ...]
    let mut rules: Vec<(u32, LabelRule)> = Vec::new();
    let mut i = 0;
    while i + 1 < nums.len() {
        let idx = match &nums[i] {
            lopdf::Object::Integer(n) if *n >= 0 => *n as u32,
            _ => return None,
        };
        let dict = match &nums[i + 1] {
            lopdf::Object::Reference(id) => doc.get_dictionary(*id).ok()?,
            lopdf::Object::Dictionary(d) => d,
            _ => return None,
        };
        if let Some(rule) = LabelRule::from_dict(dict) {
            rules.push((idx, rule));
        }
        i += 2;
    }
    rules.sort_by_key(|(idx, _)| *idx);

    let count = doc.get_pages().len();
    let mut out = Vec::with_capacity(count);
    for page_idx in 0..count {
        let rule = rules
            .iter()
            .rev()
            .find(|(start, _)| *start as usize <= page_idx);
        match rule {
            Some((start, r)) => out.push(r.render(page_idx - *start as usize)),
            None => out.push((page_idx + 1).to_string()),
        }
    }
    Some(out)
}

struct LabelRule {
    prefix: String,
    style: Option<LabelStyle>,
    start: u32,
}

enum LabelStyle {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
}

impl LabelRule {
    fn from_dict(dict: &lopdf::Dictionary) -> Option<Self> {
        let prefix = match dict.get(b"P") {
            Ok(lopdf::Object::String(s, _)) => String::from_utf8_lossy(s).into_owned(),
            _ => String::new(),
        };
        let style = match dict.get(b"S") {
            Ok(lopdf::Object::Name(n)) => match n.as_slice() {
                b"D" => Some(LabelStyle::Decimal),
                b"R" => Some(LabelStyle::UpperRoman),
                b"r" => Some(LabelStyle::LowerRoman),
                b"A" => Some(LabelStyle::UpperLetters),
                b"a" => Some(LabelStyle::LowerLetters),
                _ => None,
            },
            _ => None,
        };
        let start = match dict.get(b"St") {
            Ok(lopdf::Object::Integer(n)) if *n >= 1 => *n as u32,
            _ => 1,
        };
        Some(Self {
            prefix,
            style,
            start,
        })
    }

    fn render(&self, offset: usize) -> String {
        let n = self.start as usize + offset;
        match &self.style {
            None => self.prefix.clone(),
            Some(LabelStyle::Decimal) => format!("{}{}", self.prefix, n),
            Some(LabelStyle::UpperRoman) => {
                format!("{}{}", self.prefix, to_roman(n).to_uppercase())
            }
            Some(LabelStyle::LowerRoman) => format!("{}{}", self.prefix, to_roman(n)),
            Some(LabelStyle::UpperLetters) => {
                format!("{}{}", self.prefix, to_letters(n).to_uppercase())
            }
            Some(LabelStyle::LowerLetters) => format!("{}{}", self.prefix, to_letters(n)),
        }
    }
}

fn to_roman(mut n: usize) -> String {
    const TABLE: [(usize, &str); 7] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
    ];
    // Full table for correctness beyond 50:
    const FULL: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let _ = TABLE;
    let mut out = String::new();
    for (val, sym) in FULL {
        while n >= val {
            out.push_str(sym);
            n -= val;
        }
    }
    out
}

fn to_letters(mut n: usize) -> String {
    // Bijective base-26: 1=A .. 26=Z, 27=AA.
    let mut out = String::new();
    while n > 0 {
        n -= 1;
        out.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    out
}

/// Shared handle so rayon tasks can read the same document metadata.
pub type SharedDoc = Arc<LoadedDoc>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_leading_newlines_and_trailing_ws() {
        assert_eq!(normalize_page_text("\n\nfoo bar  \n"), "foo bar");
        assert_eq!(normalize_page_text("\n\n"), "");
        assert_eq!(normalize_page_text("a\n\nb"), "a\n\nb");
    }

    #[test]
    fn roman_numerals() {
        assert_eq!(to_roman(1), "i");
        assert_eq!(to_roman(4), "iv");
        assert_eq!(to_roman(9), "ix");
        assert_eq!(to_roman(14), "xiv");
        assert_eq!(to_roman(1994), "mcmxciv");
    }

    #[test]
    fn letter_labels() {
        assert_eq!(to_letters(1), "A");
        assert_eq!(to_letters(26), "Z");
        assert_eq!(to_letters(27), "AA");
        assert_eq!(to_letters(28), "AB");
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/").to_string() + name)
            .expect("fixture")
    }

    #[test]
    fn open_encrypted_with_password_extracts_text() {
        let bytes = fixture("t_enc.pdf");
        let doc = open(&bytes, "t_enc", &["secret".into()]).expect("open");
        assert!(doc.was_encrypted());
        let extracted = extract(&doc);
        assert_eq!(extracted.len(), 1);
        assert!(extracted.page(0).unwrap().text.contains("foo bar"));
    }

    #[test]
    fn open_encrypted_wrong_password_fails() {
        let bytes = fixture("t_enc.pdf");
        let err = open(&bytes, "t_enc", &["wrong".into()]).unwrap_err();
        assert!(matches!(err, PdfError::Encrypted { .. }), "{err:?}");
    }

    #[test]
    fn open_encrypted_no_password_fails() {
        let bytes = fixture("t_enc.pdf");
        let err = open(&bytes, "t_enc", &["".into()]).unwrap_err();
        assert!(matches!(err, PdfError::Encrypted { .. }), "{err:?}");
    }

    #[test]
    fn open_unencrypted_ignores_password_list() {
        let bytes = fixture("t.pdf");
        let doc = open(&bytes, "t", &["whatever".into()]).expect("open");
        assert!(!doc.was_encrypted());
        let extracted = extract(&doc);
        assert!(extracted.page(0).unwrap().text.contains("foo bar"));
    }
}
