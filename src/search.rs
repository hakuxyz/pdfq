//! Per-document search: Lines / CountOnly (-c,-p) / Presence (-q,-l,-L).

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
        if page_count > 0 && cfg.pagecount && cfg.only_filenames == OnlyFilenames::No && !cfg.quiet
        {
            writer.prefix(ctx, false);
            writer.raw_line(&page_count.to_string());
        }

        if cfg.max_count > 0 && state.total >= cfg.max_count {
            break;
        }
    }

    if cfg.only_filenames == OnlyFilenames::WithoutMatch && state.total == 0 && !cfg.quiet {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{OnlyFilenames, OutConf, PagenumType, Recursion};
    use crate::engine::RegexEngineImpl;
    use crate::pagerange::PageRange;
    use crate::pdf::{LoadedDoc, PageText};
    use std::sync::Arc;

    fn cfg(pattern: &str, max_count: usize, count: bool, pagenum: bool) -> Config {
        Config {
            recursion: Recursion::None,
            count,
            pagecount: false,
            quiet: false,
            passwords: vec![String::new()],
            max_count,
            debug: false,
            warn_empty: false,
            use_cache: false,
            out: OutConf {
                filename: false,
                pagenum,
                pagenum_type: PagenumType::Index,
                ..OutConf::default()
            },
            excludes: Vec::new(),
            includes: Vec::new(),
            page_range: PageRange::default(),
            only_filenames: OnlyFilenames::No,
            files: Vec::new(),
            engine: Arc::new(RegexEngineImpl::new(pattern, false).unwrap()),
        }
    }

    fn doc(lines: &[&str]) -> LoadedDoc {
        LoadedDoc::from_pages(vec![PageText {
            text: lines.join("\n"),
            label: "1".into(),
        }])
    }

    #[test]
    fn max_count_counts_matches_not_lines() {
        // One line with three matches consumes the whole budget of 3.
        let d = doc(&["foo foo foo", "foo bar", "baz foo"]);
        let out = search_document(
            &cfg("foo", 3, false, true),
            &d,
            "x.pdf",
            &cfg("foo", 3, false, true).engine,
        );
        assert_eq!(out.stdout, "1:foo foo foo\n");
        assert_eq!(out.matches, 3);
    }

    #[test]
    fn max_count_spans_lines_by_occurrence() {
        // 3 matches on line 1 + 1 on line 2 = budget 4; line 3 not reached.
        let d = doc(&["foo foo foo", "foo bar", "baz foo"]);
        let c = cfg("foo", 4, false, true);
        let out = search_document(&c, &d, "x.pdf", &c.engine);
        assert_eq!(out.stdout, "1:foo foo foo\n1:foo bar\n");
        assert_eq!(out.matches, 4);
    }

    #[test]
    fn max_count_stops_mid_line_budget() {
        // Budget 2 is fully consumed on the first line (2 of 3 matches there).
        let d = doc(&["foo foo foo", "foo bar"]);
        let c = cfg("foo", 2, false, true);
        let out = search_document(&c, &d, "x.pdf", &c.engine);
        assert_eq!(out.stdout, "1:foo foo foo\n");
        assert_eq!(out.matches, 2);
    }

    #[test]
    fn max_count_caps_count_mode() {
        let d = doc(&["foo foo foo", "foo bar", "baz foo"]);
        let c = cfg("foo", 3, true, false);
        let out = search_document(&c, &d, "x.pdf", &c.engine);
        assert_eq!(out.stdout, "3\n");
        assert_eq!(out.matches, 3);
    }

    #[test]
    fn count_without_max_counts_every_match() {
        let d = doc(&["foo foo foo", "foo bar", "baz foo"]);
        let c = cfg("foo", 0, true, false);
        let out = search_document(&c, &d, "x.pdf", &c.engine);
        assert_eq!(out.stdout, "5\n");
        assert_eq!(out.matches, 5);
    }

    #[test]
    fn max_count_one_match_per_match_occurrence_on_multi_match_line() {
        // Mirrors pdfgrep testsuite: "foofoo" alone with -m 1 prints that line.
        let d = doc(&["foofoo", "foo"]);
        let c = cfg("foo", 1, false, true);
        let out = search_document(&c, &d, "x.pdf", &c.engine);
        assert_eq!(out.stdout, "1:foofoo\n");
        assert_eq!(out.matches, 1);
    }
}
