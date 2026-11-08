//! Per-document search: page loop, match grouping, mode dispatch.
//!
//! Ported from pdfgrep's search.cc. Three modes:
//!   * Lines     - normal output, matches grouped per text line
//!   * CountOnly - only per-page and total counters (-c / -p)
//!   * Presence  - stop at first match (-q / -l / -L)

use crate::cli::{Config, OnlyFilenames};
use crate::engine::SharedEngine;
use crate::output::{MatchSpan, PrefixCtx, Writer};
use crate::pdf::{LoadedDoc, PageText};

pub struct FileOutcome {
    /// Everything this file writes to stdout (may be empty).
    pub stdout: String,
    /// stderr lines, already worded like pdfgrep's messages.
    pub stderr: Vec<String>,
    pub matches: usize,
}

impl FileOutcome {
    pub fn found(&self) -> bool {
        self.matches > 0
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Mode {
    Lines,
    CountOnly,
    Presence,
}

struct State {
    total: usize,
    document_empty: bool,
}

pub fn search_document(
    cfg: &Config,
    doc: &LoadedDoc,
    filename: &str,
    engine: &SharedEngine,
) -> FileOutcome {
    let mode = if cfg.quiet || cfg.only_filenames != OnlyFilenames::No {
        Mode::Presence
    } else if cfg.count || cfg.pagecount {
        Mode::CountOnly
    } else {
        Mode::Lines
    };

    let mut writer = Writer::new(cfg.out.clone());
    let mut state = State {
        total: 0,
        document_empty: true,
    };
    let mut stderr: Vec<String> = Vec::new();

    for idx in 0..doc.len() {
        let pagenum = (idx + 1) as u32;
        if !cfg.page_range.contains(pagenum) {
            continue;
        }
        if doc.failed_pages().contains(&pagenum) {
            if !cfg.quiet {
                stderr.push(format!("Could not search in page {pagenum} of {filename}"));
            }
            continue;
        }

        let PageText { text, label } = doc.page(idx).expect("idx < len");
        if !text.is_empty() {
            state.document_empty = false;
        }

        let ctx = PrefixCtx {
            filename,
            pagenum,
            pagelabel: label,
        };

        let page_count = search_page(cfg, &mut writer, ctx, text, engine, &mut state, mode);

        if page_count > 0 && cfg.quiet {
            break;
        }
        if cfg.only_filenames == OnlyFilenames::WithMatches && page_count > 0 {
            if !cfg.quiet {
                writer.only_filename(filename);
            }
            break;
        }
        if page_count > 0
            && cfg.pagecount
            && cfg.only_filenames == OnlyFilenames::No
            && !cfg.quiet
        {
            writer.prefix(ctx, false);
            writer.raw_line(&page_count.to_string());
        }

        if cfg.max_count > 0 && state.total >= cfg.max_count {
            break;
        }
    }

    if cfg.only_filenames == OnlyFilenames::WithoutMatch
        && state.total == 0
        && !cfg.quiet
    {
        writer.only_filename(filename);
    }

    if cfg.count && cfg.only_filenames == OnlyFilenames::No && !cfg.quiet {
        writer.prefix(
            PrefixCtx {
                filename,
                pagenum: 0,
                pagelabel: "",
            },
            false,
        );
        writer.raw_line(&state.total.to_string());
    }

    if cfg.warn_empty && state.document_empty {
        stderr.push(format!("File does not contain text: {filename}"));
    }

    FileOutcome {
        stdout: writer.take(),
        stderr,
        matches: state.total,
    }
}

fn search_page(
    cfg: &Config,
    writer: &mut Writer,
    ctx: PrefixCtx<'_>,
    text: &str,
    engine: &SharedEngine,
    state: &mut State,
    mode: Mode,
) -> usize {
    let previous_matches = state.total > 0;

    match mode {
        Mode::Presence => {
            if engine.find_at(text, 0).is_some() {
                state.total += 1;
                return 1;
            }
            return 0;
        }
        Mode::CountOnly => {
            let mut page_count = 0usize;
            let mut idx = 0usize;
            while let Some((s, e)) = engine.find_at(text, idx) {
                state.total += 1;
                page_count += 1;
                if cfg.max_count > 0 && state.total >= cfg.max_count {
                    break;
                }
                idx = if s == e { e + 1 } else { e };
                if idx >= text.len() {
                    break;
                }
            }
            return page_count;
        }
        Mode::Lines => {}
    }

    let mut page_count = 0usize;
    let mut current: Vec<MatchSpan> = Vec::new();
    let mut last_line: Vec<MatchSpan> = Vec::new();
    let mut idx = 0usize;

    while let Some((s, e)) = engine.find_at(text, idx) {
        state.total += 1;
        page_count += 1;
        let mt = MatchSpan { start: s, end: e };

        handle_match(
            cfg,
            writer,
            ctx,
            text,
            &mut current,
            &mut last_line,
            mt,
            previous_matches,
        );

        if cfg.max_count > 0 && state.total >= cfg.max_count {
            flush(
                cfg,
                writer,
                ctx,
                text,
                &mut current,
                &mut last_line,
                previous_matches,
            );
            if !last_line.is_empty() && !cfg.count && !cfg.pagecount {
                let last = *last_line.last().unwrap();
                writer.context_after(ctx, last, text);
            }
            return page_count;
        }

        idx = if s == e { e + 1 } else { e };
        if idx >= text.len() {
            break;
        }
    }

    flush(
        cfg,
        writer,
        ctx,
        text,
        &mut current,
        &mut last_line,
        previous_matches,
    );
    if !last_line.is_empty() && !cfg.count && !cfg.pagecount {
        let last = *last_line.last().unwrap();
        writer.context_after(ctx, last, text);
    }
    page_count
}

#[allow(clippy::too_many_arguments)]
fn handle_match(
    cfg: &Config,
    writer: &mut Writer,
    ctx: PrefixCtx<'_>,
    text: &str,
    current: &mut Vec<MatchSpan>,
    last_line: &mut Vec<MatchSpan>,
    mt: MatchSpan,
    previous_matches: bool,
) {
    if current.is_empty() {
        current.push(mt);
        return;
    }
    let end_last = current[current.len() - 1].end;
    let crossed = text[end_last..]
        .find('\n')
        .map(|i| end_last + i)
        .is_some_and(|nl| nl <= mt.start);
    if crossed {
        flush(cfg, writer, ctx, text, current, last_line, previous_matches);
    }
    current.push(mt);
}

fn flush(
    cfg: &Config,
    writer: &mut Writer,
    ctx: PrefixCtx<'_>,
    text: &str,
    current: &mut Vec<MatchSpan>,
    last_line: &mut Vec<MatchSpan>,
    previous_matches: bool,
) {
    let suppress = cfg.count || cfg.pagecount || cfg.only_filenames != OnlyFilenames::No;

    if !current.is_empty() && !suppress {
        if last_line.is_empty() {
            if previous_matches {
                writer.context_separator();
            }
            writer.context_before(ctx, current[0], text);
        } else {
            let prev = *last_line.last().unwrap();
            writer.context_between(ctx, prev, current[0], text);
        }

        if cfg.out.only_matching {
            for m in current.iter() {
                writer.only_match(ctx, text, *m);
            }
        } else {
            writer.line_matches(ctx, text, current);
        }
    }

    std::mem::swap(current, last_line);
    current.clear();
}
