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

    // ---- collect inputs ------------------------------------------------
    let mut files: Vec<String> = Vec::new();
    let mut walk_errors: Vec<WalkError> = Vec::new();

    // Include/exclude filters apply only to files found by directory
    // traversal; expand_arg passes explicitly named files through untouched
    // (pdfgrep passes check_excludes=false for the argument list).
    let walker = Walker::new(&cfg.includes, &cfg.excludes, cfg.recursion, true);

    let args = cfg.files.clone();
    if args.is_empty() && cfg.recursion != Recursion::None {
        walker.expand_arg(".", &mut files, &mut walk_errors);
    } else {
        for a in &args {
            walker.expand_arg(a, &mut files, &mut walk_errors);
        }
    }

    // Walk errors: pdfgrep only feeds top-level failures into the exit
    // status; problems deeper in the tree are reported and ignored.
    let mut fatal = false;
    for e in &walk_errors {
        cli::eprint(&format!("{}: {}", e.path, e.reason));
        let top_level = args.iter().any(|a| a == &e.path)
            || e.reason.starts_with("is a directory");
        if top_level {
            fatal = true;
        }
    }

    // ---- search in parallel --------------------------------------------
    // Results are printed in input order, chunk by chunk, so output stays
    // deterministic while CPU-bound extraction runs on all cores.
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

    // ---- exit status ---------------------------------------------------
    // -q: a match wins over any error, like pdfgrep.
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

    // ---- optional cache -------------------------------------------------
    let mut cached: Option<Vec<pdf::PageText>> = None;
    let mut store_cache: Option<cache::Cache> = None;
    if cfg.use_cache {
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
        None => match pdf::load(&bytes, path, &cfg.passwords) {
            Ok(d) => d.into(),
            Err(PdfError::Open { path, reason: _ }) => {
                return fail(format!("Could not open {path}"))
            }
            Err(PdfError::Encrypted { path }) => return fail(format!("Could not open {path}")),
        },
    };

    // Docs with failed page extraction are not cached so the
    // "Could not search in page ..." messages reappear on later runs
    // (pdfgrep only caches pages it could extract).
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
