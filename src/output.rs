//! Output formatting: line prefixes, match highlighting, context lines.
//!
//! Ported from pdfgrep's output.cc. Byte offsets into the page text are used
//! throughout; all indices are guaranteed to sit on char boundaries because
//! the regex engines only return valid UTF-8 spans.

use crate::cli::{OutConf, PagenumType};

/// A byte span of a match inside the page text.
#[derive(Debug, Clone, Copy)]
pub struct MatchSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy)]
pub struct PrefixCtx<'a> {
    pub filename: &'a str,
    pub pagenum: u32,
    pub pagelabel: &'a str,
}

pub struct Writer {
    buf: String,
    out: OutConf,
}

impl Writer {
    pub fn new(out: OutConf) -> Self {
        Self {
            buf: String::new(),
            out,
        }
    }

    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.buf)
    }

    fn color_on(&self, code: &str) -> String {
        if self.out.color && !code.is_empty() {
            code.to_string()
        } else {
            String::new()
        }
    }

    const COLOR_OFF: &'static str = "\x1b[m\x1b[K";

    fn color_off(&self) -> &'static str {
        if self.out.color {
            Self::COLOR_OFF
        } else {
            ""
        }
    }

    /// `filename<sep>[pagenum<sep>]` where sep is `:` normally, `-` for
    /// context lines, or NUL after the filename with `-Z`.
    pub fn prefix(&mut self, ctx: PrefixCtx<'_>, in_context: bool) {
        let sep_owned;
        let sep: &str = if in_context {
            "-"
        } else {
            sep_owned = self.out.prefix_sep.clone();
            &sep_owned
        };
        let color = self.out.color;
        let fn_c = if color {
            self.out.colors.filename.clone()
        } else {
            String::new()
        };
        let pn_c = if color {
            self.out.colors.pagenum.clone()
        } else {
            String::new()
        };
        let se_c = if color {
            self.out.colors.separator.clone()
        } else {
            String::new()
        };
        let off = if color { Self::COLOR_OFF } else { "" };

        if self.out.filename {
            if !fn_c.is_empty() {
                self.buf.push_str(&fn_c);
            }
            self.buf.push_str(ctx.filename);
            self.buf.push_str(off);

            if self.out.null_byte_sep {
                self.buf.push('\0');
            } else {
                if !se_c.is_empty() {
                    self.buf.push_str(&se_c);
                }
                self.buf.push_str(sep);
                self.buf.push_str(off);
            }
        }

        if self.out.pagenum {
            if !pn_c.is_empty() {
                self.buf.push_str(&pn_c);
            }
            match self.out.pagenum_type {
                PagenumType::Index => {
                    self.buf.push_str(&ctx.pagenum.to_string());
                }
                PagenumType::Label => {
                    self.buf.push_str(ctx.pagelabel);
                }
            }
            self.buf.push_str(off);

            if !se_c.is_empty() {
                self.buf.push_str(&se_c);
            }
            self.buf.push_str(sep);
            self.buf.push_str(off);
        }
    }

    /// Print one text line containing all matches of that line, highlighting
    /// each match. `matches` must be non-empty, sorted, on the same line.
    pub fn line_matches(&mut self, ctx: PrefixCtx<'_>, text: &str, matches: &[MatchSpan]) {
        let first = matches[0];
        let last = matches[matches.len() - 1];

        // Line start: byte after the previous newline (or 0).
        let a = text[..first.start]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        // Line end: the newline terminating the last match's line.
        let b = text[last.end..]
            .find('\n')
            .map(|i| last.end + i)
            .unwrap_or(text.len());

        let hl = self.color_on(&self.out.colors.highlight.clone());
        let off = self.color_off();

        self.prefix(ctx, false);

        let mut prev = a;
        for m in matches {
            if prev <= m.start {
                self.buf.push_str(&text[prev..m.start]);
            }
            if !hl.is_empty() {
                self.buf.push_str(&hl);
            }
            self.buf.push_str(&text[m.start..m.end]);
            self.buf.push_str(off);
            prev = m.end;
        }
        self.buf.push_str(&text[prev..b]);
        self.buf.push('\n');
    }

    /// `-o`: only the matched bytes.
    pub fn only_match(&mut self, ctx: PrefixCtx<'_>, text: &str, m: MatchSpan) {
        let hl = self.color_on(&self.out.colors.highlight.clone());
        let off = self.color_off();
        self.prefix(ctx, false);
        if !hl.is_empty() {
            self.buf.push_str(&hl);
        }
        self.buf.push_str(&text[m.start..m.end]);
        self.buf.push_str(off);
        self.buf.push('\n');
    }

    /// `-l`/`-L`: filename plus NUL or newline, never suppressed by -h.
    pub fn only_filename(&mut self, filename: &str) {
        let fn_c = self.color_on(&self.out.colors.filename.clone());
        let off = self.color_off();
        if !fn_c.is_empty() {
            self.buf.push_str(&fn_c);
        }
        self.buf.push_str(filename);
        self.buf.push_str(off);
        if self.out.null_byte_sep {
            self.buf.push('\0');
        } else {
            self.buf.push('\n');
        }
    }

    pub fn context_before(&mut self, ctx: PrefixCtx<'_>, m: MatchSpan, text: &str) {
        if !self.out.context_mode {
            return;
        }
        self.ctx_before_lines(ctx, m, text, self.out.context_before);
    }

    pub fn context_after(&mut self, ctx: PrefixCtx<'_>, m: MatchSpan, text: &str) {
        if !self.out.context_mode {
            return;
        }
        self.ctx_after_lines(ctx, m, text, self.out.context_after);
    }

    fn ctx_before_lines(
        &mut self,
        ctx: PrefixCtx<'_>,
        m: MatchSpan,
        text: &str,
        mut lines: i32,
    ) {
        let Some(line_begin) = text[..m.start].rfind('\n') else {
            return; // match is on the first line of the page
        };

        let mut collected: Vec<&str> = Vec::new();
        let mut pos = line_begin;
        while lines > 0 {
            lines -= 1;
            if pos == 0 {
                collected.push("");
                break;
            }
            let newpos = text[..pos].rfind('\n');
            let start_pos = newpos.map(|i| i + 1).unwrap_or(0);
            collected.push(&text[start_pos..pos]);
            match newpos {
                Some(np) => pos = np,
                None => break,
            }
        }

        for l in collected.iter().rev() {
            self.prefix(ctx, true);
            self.buf.push_str(l);
            self.buf.push('\n');
        }
    }

    fn ctx_after_lines(&mut self, ctx: PrefixCtx<'_>, m: MatchSpan, text: &str, mut lines: i32) {
        let Some(line_end_rel) = text[m.end..].find('\n') else {
            return; // match is on the last line of the page
        };
        let mut pos = m.end + line_end_rel;

        while lines > 0 {
            lines -= 1;
            if pos == text.len() - 1 {
                break;
            }
            let newpos = text[pos + 1..].find('\n').map(|i| pos + 1 + i);
            let end_pos = newpos.unwrap_or(text.len());
            self.prefix(ctx, true);
            self.buf.push_str(&text[pos + 1..end_pos]);
            self.buf.push('\n');
            match newpos {
                Some(np) => pos = np,
                None => break,
            }
        }
    }

    /// Between two match groups: after-context of m1, optional `--` separator,
    /// before-context of m2. Mirrors pdfgrep's print_context_between: the
    /// counting loops advance pos_right/pos_left, and `--` is printed only
    /// when lines still remain unprinted between the two blocks.
    pub fn context_between(
        &mut self,
        ctx: PrefixCtx<'_>,
        m1: MatchSpan,
        m2: MatchSpan,
        text: &str,
    ) {
        if !self.out.context_mode {
            return;
        }

        let mut pos_right: Option<usize> = text[m1.end..].find('\n').map(|i| m1.end + i);
        let mut pos_left: Option<usize> = text[..m2.start].rfind('\n');

        // Count lines after m1 that fall strictly before m2's line.
        let mut lines_right = 0i32;
        while let Some(pr) = pos_right {
            if lines_right >= self.out.context_after {
                break;
            }
            match pos_left {
                Some(pl) if pr < pl => {}
                _ => break,
            }
            lines_right += 1;
            pos_right = text[pr + 1..].find('\n').map(|i| pr + 1 + i);
        }

        // Count lines before m2 that fall strictly after m1's line.
        let mut lines_left = 0i32;
        while let Some(pl) = pos_left {
            if lines_left >= self.out.context_before {
                break;
            }
            match pos_right {
                Some(pr) if pl > pr => {}
                _ => break,
            }
            lines_left += 1;
            if pl == 0 {
                pos_left = None;
                break;
            }
            pos_left = text[..pl].rfind('\n');
        }

        self.ctx_after_lines(ctx, m1, text, lines_right);
        // pdfgrep compares the mutated positions (npos semantics: None on the
        // left still separates, None on the right does not).
        let sep = match (pos_left, pos_right) {
            (Some(pl), Some(pr)) => pl > pr,
            (None, Some(_)) => true,
            _ => false,
        };
        if sep {
            self.context_separator();
        }
        self.ctx_before_lines(ctx, m2, text, lines_left);
    }

    pub fn context_separator(&mut self) {
        if self.out.context_mode {
            self.buf.push_str("--\n");
        }
    }

    /// Raw push used for count/page-count lines.
    pub fn raw_line(&mut self, s: &str) {
        self.buf.push_str(s);
        self.buf.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ColorScheme;

    fn conf() -> OutConf {
        OutConf {
            filename: false,
            pagenum: false,
            pagenum_type: PagenumType::Index,
            color: false,
            only_matching: false,
            null_byte_sep: false,
            prefix_sep: ":".into(),
            context_mode: false,
            context_before: 0,
            context_after: 0,
            colors: ColorScheme::default(),
        }
    }

    fn ctx<'a>() -> PrefixCtx<'a> {
        PrefixCtx {
            filename: "f.pdf",
            pagenum: 1,
            pagelabel: "i",
        }
    }

    #[test]
    fn plain_line_with_match() {
        let mut w = Writer::new(conf());
        let text = "hello world";
        w.line_matches(ctx(), text, &[MatchSpan { start: 6, end: 11 }]);
        // The whole line is printed, only the match is highlighted.
        assert_eq!(w.take(), "hello world\n");
    }

    #[test]
    fn page_number_prefix() {
        let mut c = conf();
        c.pagenum = true;
        let mut w = Writer::new(c);
        w.line_matches(ctx(), "abc", &[MatchSpan { start: 0, end: 3 }]);
        assert_eq!(w.take(), "1:abc\n");
    }

    #[test]
    fn filename_and_page_prefix() {
        let mut c = conf();
        c.filename = true;
        c.pagenum = true;
        let mut w = Writer::new(c);
        w.line_matches(ctx(), "abc", &[MatchSpan { start: 0, end: 3 }]);
        assert_eq!(w.take(), "f.pdf:1:abc\n");
    }

    #[test]
    fn null_separator_after_filename_only() {
        let mut c = conf();
        c.filename = true;
        c.pagenum = true;
        c.null_byte_sep = true;
        let mut w = Writer::new(c);
        w.line_matches(ctx(), "abc", &[MatchSpan { start: 0, end: 3 }]);
        assert_eq!(w.take(), "f.pdf\01:abc\n");
    }

    #[test]
    fn context_uses_dash_separator() {
        let mut c = conf();
        c.filename = true;
        c.pagenum = true;
        c.context_mode = true;
        c.context_before = 1;
        c.context_after = 1;
        let mut w = Writer::new(c);
        let text = "one\ntwo\nthree";
        // match on "two"
        w.prefix(ctx(), true);
        w.raw_line("one");
        w.line_matches(ctx(), text, &[MatchSpan { start: 4, end: 7 }]);
        w.prefix(ctx(), true);
        w.raw_line("three");
        // Context lines join every part with "-"; only match lines use ":".
        assert_eq!(w.take(), "f.pdf-1-one\nf.pdf:1:two\nf.pdf-1-three\n");
    }

    #[test]
    fn multiple_matches_same_line() {
        let mut w = Writer::new(conf());
        let text = "foofoo";
        w.line_matches(
            ctx(),
            text,
            &[MatchSpan { start: 0, end: 3 }, MatchSpan { start: 3, end: 6 }],
        );
        assert_eq!(w.take(), "foofoo\n");
    }
}
