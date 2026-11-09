# pdfq — Promotional texts

## Hacker News (Show HN)

**Title (≤80 chars):**

```
Show HN: pdfq – Fast parallel PDF search, drop-in for pdfgrep (Rust)
```

**First comment / self-post body:**

```
I rewrote pdfgrep in Rust as pdfq. Same flags, same output format, same
exit codes (0/1/2), so it can replace pdfgrep in scripts without edits.

Why:
- Parallel page + file search (rayon) — often noticeably faster on multi-core
- Pure Rust stack: pdf-extract + regex/fancy-regex, no poppler/system deps
- Optional --cache of extracted text for repeated searches on big PDFs
- Differential test suite that diffs stdout/stderr/exit codes against real
  pdfgrep 2.2.0 (50+ cases), plus unit/integration tests

Install:
  cargo install pdfq

  pdfq -n -i "error" ./reports
  pdfq -r -c "TODO" .
  pdfq -m 3 -C2 "summary" thesis.pdf

Repo: https://github.com/hakuxyz/pdfq
crates.io: https://crates.io/crates/pdfq
docs: https://docs.rs/pdfq

Honest caveats (from the README):
- Text extraction uses pdf-extract, not poppler — layout can differ on messy PDFs
- Rust regex `.` does not match newline (differs from POSIX grep)
- No --unac accent folding

Would love feedback on edge cases where pdfgrep and pdfq disagree — the
difftest harness makes those easy to reproduce.
```

**Shorter HN title alternatives:**

```
Show HN: pdfgrep-compatible PDF search in Rust (parallel, pure Rust)
Show HN: cargo install pdfq – drop-in pdfgrep replacement
pdfq: parallel PDF search with a pdfgrep-compatible CLI
```

---

## Reddit r/commandline or r/rust

**Title:**

```
pdfq – parallel PDF search, drop-in pdfgrep replacement (Rust, MIT)
```

**Body:**

```
I built pdfq: a from-scratch Rust rewrite of pdfgrep.

  cargo install pdfq
  pdfq -rn -i "TODO" ./docs

- pdfgrep-compatible CLI (-i, -n, -c, -o, -l, -q, -F, -P, -A/-B/-C, -m, -r, --color, …)
- GNU-grep exit codes for scripting
- Rayon parallelism across pages and files
- Optional on-disk cache (--cache) for repeated runs
- Differential tests against pdfgrep itself

GitHub: https://github.com/hakuxyz/pdfq
crates.io: https://crates.io/crates/pdfq
```

---

## Twitter / Mastodon / Bluesky

**Post:**

```
New crate: pdfq — fast, parallel PDF search with a pdfgrep-compatible CLI.

  cargo install pdfq
  pdfq -n -i "error" report.pdf

Pure Rust (no poppler), rayon parallel, optional text cache, exit codes 0/1/2
like grep. Drop-in for pdfgrep in scripts.

https://crates.io/crates/pdfq
https://github.com/hakuxyz/pdfq

#rustlang #pdf #cli
```

**Short:**

```
cargo install pdfq  # parallel PDF search, pdfgrep-compatible CLI in Rust
```

---

## Lobsters / HN “Ask” style

```
pdfgrep in Rust: I care more about matching pdfgrep’s quirks than about
being “better” than it — has anyone hit cases where a rewrite should
diverge? Repo has a difftest.sh that diffs against the real binary.
```

---

## Reddit r/rust weekly (if allowed)

```
I released pdfq 0.1.1 on crates.io: parallel PDF search with pdfgrep’s CLI.
Differential tests compare stdout/stderr/exit codes against pdfgrep 2.2.0.
Feedback welcome, especially on extraction differences (pdf-extract vs poppler).
```

---

## One-liners (for link posts / profiles)

- `Fast parallel PDF search — pdfgrep-compatible CLI in pure Rust. cargo install pdfq`
- `pdfq: drop-in pdfgrep replacement, rayon-parallel, MIT.`
- `Same flags as pdfgrep. Faster cores. No poppler.`
