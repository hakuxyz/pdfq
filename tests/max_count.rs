//! `-m` / `--max-count` limits match occurrences (pdfgrep semantics), not lines.

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

fn run(args: &[&str]) -> (String, String, Option<i32>) {
    let out = pdfq().args(args).output().expect("run pdfq");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

#[test]
fn multi_match_line_consumes_multiple_budget() {
    // multiline.pdf line 1 is "foo foo foo" (3 matches), then two more lines.
    let f = fixture("multiline.pdf");
    let f = f.to_str().unwrap();

    // Budget 3 is exhausted on line 1 alone; later lines are not printed.
    let (stdout, _, code) = run(&["-n", "--color=never", "-m", "3", "-e", "foo", f]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout, "1:foo foo foo\n");

    // Budget 4: line 1 (3 matches) + first match on line 2.
    let (stdout, _, code) = run(&["-n", "--color=never", "-m", "4", "-e", "foo", f]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout, "1:foo foo foo\n1:foo bar\n");

    // -c with -m never reports more than the budget.
    let (stdout, _, code) = run(&["--color=never", "-c", "-m", "2", "-e", "foo", f]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "2");
}

#[test]
fn count_mode_without_max_counts_all_matches() {
    let f = fixture("multiline.pdf");
    let f = f.to_str().unwrap();
    let (stdout, _, code) = run(&["--color=never", "-c", "-e", "foo", f]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "5");
}

#[test]
fn max_count_one_on_double_match_line() {
    // multicount.pdf: "foofoo" (2 matches) then "foo"; pdfgrep suite expects only first line for -m 1.
    let f = fixture("multicount.pdf");
    let f = f.to_str().unwrap();
    let (stdout, _, code) = run(&["-n", "--color=never", "-m", "1", "-e", "foo", f]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout, "1:foofoo\n");

    let (stdout, _, code) = run(&["--color=never", "-c", "-e", "foo", f]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim(), "4");
}

#[test]
fn help_says_matches_not_lines() {
    let (stdout, _, _) = run(&["--help"]);
    assert!(
        stdout.contains("Stop reading after NUM matches"),
        "help should describe match-occurrence semantics:\n{stdout}"
    );
}
