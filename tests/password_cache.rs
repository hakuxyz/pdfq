//! Regression: `--cache` must not bypass the password check on encrypted PDFs.

use std::path::PathBuf;
use std::process::Command;

fn pdfq() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdfq"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn cache_does_not_bypass_password() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = dir.path().join("cache");
    let enc = fixture("t_enc.pdf");

    // Correct password + --cache: match, but nothing is cached for encrypted docs.
    let ok = pdfq()
        .env("PDFQ_CACHE_DIR", &cache)
        .args([
            "--cache",
            "--password",
            "secret",
            "-n",
            "--color=never",
            "-e",
            "foo",
        ])
        .arg(&enc)
        .output()
        .expect("run");
    assert_eq!(
        ok.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ok.stdout).contains("foo bar"),
        "stdout={}",
        String::from_utf8_lossy(&ok.stdout)
    );
    // Encrypted documents must not write cache entries.
    assert!(
        !cache.exists() || cache.read_dir().map(|d| d.count()).unwrap_or(0) == 0,
        "cache dir must stay empty for encrypted PDFs"
    );

    // Wrong password + --cache: still fails (open happens before any cache read).
    let bad = pdfq()
        .env("PDFQ_CACHE_DIR", &cache)
        .args([
            "--cache",
            "--password",
            "wrong",
            "-n",
            "--color=never",
            "-e",
            "foo",
        ])
        .arg(&enc)
        .output()
        .expect("run");
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("Could not open"));

    // No password + --cache: same failure.
    let none = pdfq()
        .env("PDFQ_CACHE_DIR", &cache)
        .args(["--cache", "-n", "--color=never", "-e", "foo"])
        .arg(&enc)
        .output()
        .expect("run");
    assert_eq!(none.status.code(), Some(2));
}

#[test]
fn cache_still_works_for_unencrypted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = dir.path().join("cache");
    let plain = fixture("t.pdf");

    for _ in 0..2 {
        let out = pdfq()
            .env("PDFQ_CACHE_DIR", &cache)
            .args(["--cache", "-n", "--color=never", "-e", "foo"])
            .arg(&plain)
            .output()
            .expect("run");
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("foo bar"));
    }

    let entries: Vec<_> = cache.read_dir().expect("cache created").collect();
    assert_eq!(entries.len(), 1, "one cache entry for one file");
}
