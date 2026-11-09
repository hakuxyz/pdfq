//! pdfq: fast, parallel PDF search compatible with pdfgrep's CLI.

mod cache;
mod cli;
mod engine;
mod output;
mod pagerange;
mod pdf;
mod search;
mod walk;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

use cli::{Config, Recursion, EXIT_ERROR, EXIT_NO_MATCH, EXIT_OK};
use pdf::{PdfError, SharedDoc};
use search::FileOutcome;
use walk::{WalkError, Walker};

fn main() {
    let cmd = cli::build_cli();
    let matches = match cmd.try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            let _ = e.print();
            std::process::exit(if e.use_stderr() { EXIT_ERROR } else { EXIT_OK });
        }
    };

    let cfg = cli::build_config(&matches);

    // collect input files
    let mut files: Vec<String> = Vec::new();
    let mut walk_errors: Vec<WalkError> = Vec::new();

    // Filters apply only during -r expansion, not to explicit CLI files.
    let walker = Walker::new(&cfg.includes, &cfg.excludes, cfg.recursion, true);

    let args = cfg.files.clone();
    if args.is_empty() && cfg.recursion != Recursion::None {
        walker.expand_arg(".", &mut files, &mut walk_errors);
    } else {
        for a in &args {
            walker.expand_arg(a, &mut files, &mut walk_errors);
        }
    }

    // Only top-level walk errors affect the exit status (like pdfgrep).
    let mut fatal = false;
    for e in &walk_errors {
        cli::eprint(&format!("{}: {}", e.path, e.reason));
        let top_level = args.iter().any(|a| a == &e.path)
            || e.reason.starts_with("is a directory");
        if top_level {
            fatal = true;
        }
    }

    // Parallel search; results printed in input order for determinism.
    let any_found = AtomicBool::new(false);
    let mut had_open_error = false;
    let chunk = std::thread::available_parallelism()
        .map(|n| n.get() * 8)
        .unwrap_or(32);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for part in files.chunks(chunk) {
        if cfg.quiet && any_found.load(Ordering::Relaxed) {
            break;
        }
        let results: Vec<Processed> = part
            .par_iter()
            .map(|path| process_file(&cfg, path))
            .collect();

        for (path, res) in part.iter().zip(results.iter()) {
            if let Some(msg) = &res.open_error {
                cli::eprint(msg);
                if args.iter().any(|a| a == path) {
                    had_open_error = true;
                }
            }
            for line in &res.outcome.stderr {
                cli::eprint(line);
            }
            if res.found() {
                any_found.store(true, Ordering::Relaxed);
            }
            if !cfg.quiet {
                let _ = out.write_all(res.outcome.stdout.as_bytes());
            }
        }
    }
    let _ = out.flush();

    // -q: a match wins over errors, like pdfgrep.
    if cfg.quiet && any_found.load(Ordering::Relaxed) {
        std::process::exit(EXIT_OK);
    }
    if fatal || had_open_error {
        std::process::exit(EXIT_ERROR);
    }
    if any_found.load(Ordering::Relaxed) {
        std::process::exit(EXIT_OK);
    }
    std::process::exit(EXIT_NO_MATCH);
}

struct Processed {
    outcome: FileOutcome,
    open_error: Option<String>,
}

impl Processed {
    fn found(&self) -> bool {
        self.outcome.found()
    }
}

fn process_file(cfg: &Config, path: &str) -> Processed {
    let fail = |msg: String| Processed {
        outcome: FileOutcome {
            stdout: String::new(),
            stderr: Vec::new(),
            matches: 0,
        },
        open_error: Some(msg),
    };

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        // Wording matches pdfgrep: no OS reason appended.
        Err(_) => return fail(format!("Could not open {path}")),
    };

    // Open+decrypt first so cache hits cannot bypass the password check.
    let opened = match pdf::open(&bytes, path, &cfg.passwords) {
        Ok(d) => d,
        Err(PdfError::Open { path, reason: _ }) => {
            return fail(format!("Could not open {path}"))
        }
        Err(PdfError::Encrypted { path }) => return fail(format!("Could not open {path}")),
    };
    // was_encrypted stays true after a password load clears the Encrypt trailer entry.
    let encrypted = opened.was_encrypted();

    // Cache only unencrypted docs; never persist decrypted page text.
    let mut cached: Option<Vec<pdf::PageText>> = None;
    let mut store_cache: Option<cache::Cache> = None;
    if cfg.use_cache && !encrypted {
        if let Some(c) = cache::Cache::open(&bytes) {
            cached = c.load();
            if cached.is_none() {
                store_cache = Some(c);
            }
        } else if cfg.debug {
            return fail(format!("Could not initialize cache for {path}"));
        }
    }

    let doc: SharedDoc = match cached {
        Some(pages) => pdf::LoadedDoc::from_pages(pages).into(),
        None => pdf::extract(&opened).into(),
    };

    // Skip cache when a page failed to extract so the error can reappear.
    if let Some(c) = store_cache
        && doc.failed_pages().is_empty()
    {
        c.store(&doc.pages_for_cache());
    }

    Processed {
        outcome: search::search_document(cfg, &doc, path, &cfg.engine),
        open_error: None,
    }
}
