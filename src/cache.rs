//! Optional on-disk cache for extracted page text (`--cache`).
//!
//! Keyed by SHA-256 of the raw file bytes so any change to the PDF
//! invalidates the entry. Format is a tiny hand-rolled binary layout; no
//! serde needed for four length-prefixed fields per page.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::pdf::PageText;

const MAGIC: &[u8; 8] = b"PDFQCA01";
const MAX_ENTRIES: u64 = 200;
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Cache {
    dir: PathBuf,
    key: String,
}

fn cache_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("PDFQ_CACHE_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Ok(x) = std::env::var("XDG_CACHE_HOME")
        && !x.is_empty() {
            return Some(PathBuf::from(x).join("pdfq"));
        }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".cache/pdfq"))
}

impl Cache {
    /// Returns None when the cache directory cannot be created; the caller
    /// then just runs without caching (pdfgrep warns, we stay quiet unless
    /// --debug was requested by the caller printing our None reason).
    pub fn open(bytes: &[u8]) -> Option<Self> {
        let dir = cache_dir()?;
        fs::create_dir_all(&dir).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let key = hex(&hasher.finalize());
        Some(Self { dir, key })
    }

    fn path(&self) -> PathBuf {
        self.dir.join(&self.key)
    }

    pub fn load(&self) -> Option<Vec<PageText>> {
        let data = fs::read(self.path()).ok()?;
        decode(&data)
    }

    pub fn store(&self, pages: &[PageText]) {
        let data = encode(pages);
        let tmp = self.dir.join(format!("{}.tmp", self.key));
        if fs::write(&tmp, &data).is_ok() {
            let _ = fs::rename(&tmp, self.path());
            prune(&self.dir);
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode(pages: &[PageText]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(pages.len() as u32).to_le_bytes());
    for p in pages {
        write_str(&mut out, &p.label);
        write_str(&mut out, &p.text);
    }
    out
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn decode(data: &[u8]) -> Option<Vec<PageText>> {
    if data.len() < 12 || &data[..8] != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let mut pos = 12;
    let mut pages = Vec::with_capacity(count);
    for _ in 0..count {
        let label = read_str(data, &mut pos)?;
        let text = read_str(data, &mut pos)?;
        pages.push(PageText { text, label });
    }
    Some(pages)
}

fn read_str(data: &[u8], pos: &mut usize) -> Option<String> {
    let len_bytes: [u8; 4] = data.get(*pos..*pos + 4)?.try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    *pos += 4;
    let end = *pos + len;
    let s = std::str::from_utf8(data.get(*pos..end)?).ok()?;
    *pos = end;
    Some(s.to_string())
}

/// Keep at most MAX_ENTRIES files, drop anything older than a day.
fn prune(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<(SystemTime, PathBuf)> = Vec::new();
    for ent in rd.flatten() {
        let Ok(md) = ent.metadata() else { continue };
        let Ok(mtime) = md.modified() else { continue };
        entries.push((mtime, ent.path()));
    }
    let now = SystemTime::now();
    for (mtime, path) in &entries {
        if now.duration_since(*mtime).unwrap_or_default() > MAX_AGE {
            let _ = fs::remove_file(path);
        }
    }
    entries.retain(|(_, p)| p.exists());
    if entries.len() as u64 > MAX_ENTRIES {
        entries.sort_by_key(|(t, _)| *t);
        let excess = entries.len() as u64 - MAX_ENTRIES;
        for (_, path) in entries.iter().take(excess as usize) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let pages = vec![
            PageText {
                text: "hello\nworld".into(),
                label: "1".into(),
            },
            PageText {
                text: String::new(),
                label: "ii".into(),
            },
        ];
        let encoded = encode(&pages);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].text, "hello\nworld");
        assert_eq!(decoded[1].label, "ii");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not a cache file at all").is_none());
        let mut good = encode(&[]);
        good.push(0xff);
        // still decodes: trailing junk ignored
        assert!(decode(&good).is_some());
    }

    #[test]
    fn io_error_is_not_fatal() {
        // Point the cache dir at a path that cannot be created; open must
        // return None instead of panicking. set_var is unsafe in edition 2024
        // because it races with concurrent getenv - accepted here since no
        // other test touches PDFQ_CACHE_DIR at the same time.
        unsafe {
            std::env::set_var("PDFQ_CACHE_DIR", "/dev/null/x");
        }
        assert!(Cache::open(b"abc").is_none());
        unsafe {
            std::env::remove_var("PDFQ_CACHE_DIR");
        }
    }
}
