//! CLI parsing. `-h` is no-filename; with `-e`/`-f` all positionals are files.

use std::path::Path;

use clap::{Arg, ArgAction, Command};

use crate::engine::{FixedEngine, NeverEngine, PcreEngine, PatternList, RegexEngineImpl, SharedEngine};
use crate::pagerange::PageRange;

pub const EXIT_OK: i32 = 0;
pub const EXIT_NO_MATCH: i32 = 1;
pub const EXIT_ERROR: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recursion {
    None,
    DontFollowSymlinks,
    FollowSymlinks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagenumType {
    Index,
    Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlyFilenames {
    No,
    WithMatches,
    WithoutMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorWhen {
    Always,
    Never,
    Auto,
}

/// Pre-escaped SGR sequences, empty = disabled.
#[derive(Debug, Clone, Default)]
pub struct ColorScheme {
    pub filename: String,
    pub pagenum: String,
    pub highlight: String,
    pub separator: String,
}

#[derive(Debug, Clone)]
pub struct OutConf {
    pub filename: bool,
    pub pagenum: bool,
    pub pagenum_type: PagenumType,
    pub color: bool,
    pub only_matching: bool,
    pub null_byte_sep: bool,
    pub prefix_sep: String,
    pub context_mode: bool,
    pub context_before: i32,
    pub context_after: i32,
    pub colors: ColorScheme,
}

impl Default for OutConf {
    fn default() -> Self {
        Self {
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
            colors: default_colors(),
        }
    }
}

fn default_colors() -> ColorScheme {
    // grep(1) defaults for GREP_COLORS.
    ColorScheme {
        filename: sgr("35"),
        pagenum: sgr("32"),
        highlight: sgr("01;31"),
        separator: sgr("36"),
    }
}

fn sgr(code: &str) -> String {
    format!("\x1b[{code}m\x1b[K")
}

pub struct Config {
    pub recursion: Recursion,
    pub count: bool,
    pub pagecount: bool,
    pub quiet: bool,
    pub passwords: Vec<String>,
    pub max_count: usize,
    pub debug: bool,
    pub warn_empty: bool,
    pub use_cache: bool,
    pub out: OutConf,
    pub excludes: Vec<String>,
    pub includes: Vec<String>,
    pub page_range: PageRange,
    pub only_filenames: OnlyFilenames,
    pub files: Vec<String>,
    pub engine: SharedEngine,
}

pub fn build_cli() -> Command {
    Command::new("pdfq")
        .about("Search for PATTERN in each FILE.\nPATTERN is, by default, an extended regular expression.")
        .override_usage("pdfq [OPTION]... PATTERN FILE...")
        .version(env!("CARGO_PKG_VERSION"))
        .disable_version_flag(true)
        .disable_help_flag(true)
        .arg(
            Arg::new("help")
                .long("help")
                .action(ArgAction::Help)
                .help("Print this help"),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help("Show version information"),
        )
        .arg(flag('i', "ignore-case", "Ignore case distinctions"))
        .arg(
            Arg::new("perl-regexp")
                .short('P')
                .long("perl-regexp")
                .action(ArgAction::SetTrue)
                .help("Use Perl compatible regular expressions (PCRE)"),
        )
        .arg(
            Arg::new("page-number")
                .short('n')
                .long("page-number")
                .num_args(0..=1)
                .default_missing_value("index")
                .value_parser(["index", "label"])
                .value_name("TYPE")
                .help("Print page number with output lines (TYPE: index or label)"),
        )
        .arg(
            Arg::new("with-filename")
                .short('H')
                .long("with-filename")
                .action(ArgAction::SetTrue)
                .help("Print the file name for each match"),
        )
        .arg(
            Arg::new("no-filename")
                .short('h')
                .long("no-filename")
                .action(ArgAction::SetTrue)
                .help("Suppress the prefixing of file name on output"),
        )
        .arg(flag('c', "count", "Print only a count of matches per file"))
        .arg(
            Arg::new("color")
                .long("color")
                .value_name("WHEN")
                .value_parser(["always", "never", "auto"])
                .help("Use colors: always, never or auto"),
        )
        .arg(flag('r', "recursive", "Search directories recursively"))
        .arg(
            Arg::new("dereference-recursive")
                .short('R')
                .long("dereference-recursive")
                .action(ArgAction::SetTrue)
                .help("Like -r, but follow all symlinks"),
        )
        .arg(
            Arg::new("exclude")
                .long("exclude")
                .value_name("GLOB")
                .action(ArgAction::Append)
                .help("Skip files whose base name matches GLOB"),
        )
        .arg(
            Arg::new("include")
                .long("include")
                .value_name("GLOB")
                .action(ArgAction::Append)
                .help("Only search files whose base name matches GLOB"),
        )
        .arg(flag('p', "page-count", "Print only a count of matches per page"))
        .arg(flag('q', "quiet", "Suppress normal output"))
        .arg(
            Arg::new("password")
                .long("password")
                .value_name("PASSWORD")
                .action(ArgAction::Append)
                .help("Password to decrypt PDF files (repeatable)"),
        )
        .arg(
            Arg::new("max-count")
                .short('m')
                .long("max-count")
                .value_name("NUM")
                .value_parser(parse_max_count)
                .help("Stop reading after NUM matching lines (per file)"),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .action(ArgAction::SetTrue)
                .help("Enable debug output"),
        )
        .arg(
            Arg::new("only-matching")
                .short('o')
                .long("only-matching")
                .action(ArgAction::SetTrue)
                .help("Print only the matched part of a line"),
        )
        .arg(
            Arg::new("null")
                .short('Z')
                .long("null")
                .action(ArgAction::SetTrue)
                .help("Output a null byte instead of the filename separator"),
        )
        .arg(
            Arg::new("match-prefix-separator")
                .long("match-prefix-separator")
                .value_name("SEP")
                .help("Change the colon separator in the output"),
        )
        .arg(
            Arg::new("warn-empty")
                .long("warn-empty")
                .action(ArgAction::SetTrue)
                .help("Warn if a PDF contains no searchable text"),
        )
        .arg(
            Arg::new("unac")
                .long("unac")
                .action(ArgAction::SetTrue)
                .help("Remove accents from pattern and documents"),
        )
        .arg(
            Arg::new("fixed-strings")
                .short('F')
                .long("fixed-strings")
                .action(ArgAction::SetTrue)
                .help("Interpret PATTERN as a list of fixed strings"),
        )
        .arg(
            Arg::new("cache")
                .long("cache")
                .action(ArgAction::SetTrue)
                .help("Use a cache for extracted page text"),
        )
        .arg(
            Arg::new("after-context")
                .short('A')
                .long("after-context")
                .value_name("NUM")
                .value_parser(parse_ctx_num)
                .help("Print NUM lines of context after matching lines"),
        )
        .arg(
            Arg::new("before-context")
                .short('B')
                .long("before-context")
                .value_name("NUM")
                .value_parser(parse_ctx_num)
                .help("Print NUM lines of context before matching lines"),
        )
        .arg(
            Arg::new("context")
                .short('C')
                .long("context")
                .value_name("NUM")
                .value_parser(parse_ctx_num)
                .help("Print NUM lines of context before and after"),
        )
        .arg(
            Arg::new("page-range")
                .long("page-range")
                .value_name("RANGE")
                .help("Limit search to pages, e.g. 2-3,5,7-10"),
        )
        .arg(
            Arg::new("regexp")
                .short('e')
                .long("regexp")
                .value_name("PATTERN")
                .action(ArgAction::Append)
                .allow_hyphen_values(true)
                .help("Use PATTERN; may be given multiple times"),
        )
        .arg(
            Arg::new("file")
                .short('f')
                .long("file")
                .value_name("FILE")
                .action(ArgAction::Append)
                .allow_hyphen_values(true)
                .help("Read patterns from FILE, one per line"),
        )
        .arg(
            Arg::new("files-with-matches")
                .short('l')
                .long("files-with-matches")
                .action(ArgAction::SetTrue)
                .help("Print only names of files with matches"),
        )
        .arg(
            Arg::new("files-without-match")
                .short('L')
                .long("files-without-match")
                .action(ArgAction::SetTrue)
                .help("Print only names of files without matches"),
        )
        // Catch-all positional: split into pattern vs files in build_config().
        .arg(
            Arg::new("args")
                .value_name("PATTERN|FILE")
                .num_args(0..)
                .action(ArgAction::Append),
        )
}

fn flag(short: char, long: &'static str, help: &'static str) -> Arg {
    Arg::new(long)
        .short(short)
        .long(long)
        .action(ArgAction::SetTrue)
        .help(help)
}

/// Print pdfgrep's short usage to stdout and exit 2.
pub fn print_usage_and_exit() -> ! {
    println!(
        "Usage: pdfq [OPTION]... PATTERN FILE...\n\nSee 'pdfq --help' for more information"
    );
    std::process::exit(EXIT_ERROR);
}

pub fn eprint(msg: &str) {
    eprintln!("pdfq: {msg}");
}

/// Build Config from clap matches; exits 2 on usage/config errors.
pub fn build_config(m: &clap::ArgMatches) -> Config {
    if m.get_flag("unac") {
        eprint("--unac support is not available in this build");
        std::process::exit(EXIT_ERROR);
    }
    if m.get_flag("perl-regexp") && m.get_flag("fixed-strings") {
        eprint("--perl-regexp and --fixed-strings cannot be used together");
        std::process::exit(EXIT_ERROR);
    }

    // pattern vs file positionals
    let explicit_e: Vec<String> = multi_values(m, "regexp");
    let explicit_f: Vec<String> = multi_values(m, "file");
    let has_explicit_patterns = !explicit_e.is_empty() || !explicit_f.is_empty();

    // -e/-f: all positionals are files; else first is pattern, rest files.
    let positionals: Vec<String> = multi_values(m, "args");
    let (pattern_pos, mut files): (Option<String>, Vec<String>) = if has_explicit_patterns {
        (None, positionals)
    } else {
        let mut it = positionals.into_iter();
        (it.next(), it.collect())
    };

    let recursion = if m.get_flag("dereference-recursive") {
        Recursion::FollowSymlinks
    } else if m.get_flag("recursive") {
        Recursion::DontFollowSymlinks
    } else {
        Recursion::None
    };

    // Need a pattern (unless -e/-f) and a target (unless -r), like pdfgrep.
    let n_positional = pattern_pos.is_some() as usize + files.len();
    let required = if has_explicit_patterns { 0 } else { 1 }
        + if recursion == Recursion::None { 1 } else { 0 };
    if n_positional < required {
        print_usage_and_exit();
    }

    let mut patterns: Vec<String> = explicit_e;
    for pf in &explicit_f {
        match std::fs::read_to_string(pf) {
            Ok(content) => {
                // getline: empty file = no patterns; split on \n, drop trailing artifact.
                if content.is_empty() {
                    continue;
                }
                let mut lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
                if content.ends_with('\n') {
                    lines.pop();
                }
                patterns.extend(lines);
            }
            Err(e) => {
                eprint(&format!("{pf}: {e}"));
                std::process::exit(EXIT_ERROR);
            }
        }
    }

    if !has_explicit_patterns {
        // first positional = pattern, rest = files
        patterns.push(pattern_pos.clone().unwrap());
    } else if let Some(p) = pattern_pos {
        files.push(p);
    }

    // build regex engine
    let ignore_case = m.get_flag("ignore-case");
    let engine = build_engine(&patterns, ignore_case, m.get_flag("fixed-strings"), m.get_flag("perl-regexp"));

    // output settings
    let mut out = OutConf::default();

    let color_when = match m.get_one::<String>("color").map(|s| s.as_str()) {
        Some("always") => ColorWhen::Always,
        Some("never") => ColorWhen::Never,
        _ => ColorWhen::Auto,
    };
    let color_tty = std::io::IsTerminal::is_terminal(&std::io::stdout())
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(false);
    out.color = match color_when {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        ColorWhen::Auto => color_tty,
    };
    if out.color {
        read_grep_colors(&mut out.colors);
    }

    let mut explicit_filename = false;
    if m.get_flag("with-filename") {
        out.filename = true;
        explicit_filename = true;
    }
    if m.get_flag("no-filename") {
        out.filename = false;
        explicit_filename = true;
    }

    if let Some(t) = m.get_one::<String>("page-number") {
        out.pagenum = true;
        out.pagenum_type = if t == "label" {
            PagenumType::Label
        } else {
            PagenumType::Index
        };
    }

    let count = m.get_flag("count");
    let pagecount = m.get_flag("page-count");
    if pagecount {
        out.pagenum = true;
    }
    if count && out.pagenum {
        eprint("warning: --count and --page-number can't be used together. Ignoring --page-number.");
        out.pagenum = false;
    }

    if let Some(sep) = m.get_one::<String>("match-prefix-separator") {
        out.prefix_sep = sep.clone();
    }
    out.null_byte_sep = m.get_flag("null");
    out.only_matching = m.get_flag("only-matching");

    if let Some(n) = m.get_one::<i32>("after-context") {
        out.context_after = *n;
        out.context_mode = true;
    }
    if let Some(n) = m.get_one::<i32>("before-context") {
        out.context_before = *n;
        out.context_mode = true;
    }
    if let Some(n) = m.get_one::<i32>("context") {
        out.context_after = *n;
        out.context_before = *n;
        out.context_mode = true;
    }
    if out.only_matching && out.context_mode {
        eprint("warning: --only-matching and context options can't be used together. Ignoring context option.");
        out.context_mode = false;
        out.context_before = 0;
        out.context_after = 0;
    }

    // default filename display
    if !explicit_filename {
        out.filename = !(files.len() == 1 && !Path::new(&files[0]).is_dir());
    }

    let only_filenames = if m.get_flag("files-with-matches") {
        OnlyFilenames::WithMatches
    } else if m.get_flag("files-without-match") {
        OnlyFilenames::WithoutMatch
    } else {
        OnlyFilenames::No
    };

    let mut includes: Vec<String> = multi_values(m, "include");
    if includes.is_empty() {
        includes.push("*.[Pp][Dd][Ff]".into());
    }
    let excludes: Vec<String> = multi_values(m, "exclude");

    let page_range = match m.get_one::<String>("page-range") {
        Some(input) => match PageRange::parse(input) {
            Ok(r) => {
                if let Some(w) = crate::pagerange::empty_range_warning(&r, input) {
                    eprint(&w);
                }
                r
            }
            Err(e) => {
                eprint(&e.to_string());
                std::process::exit(EXIT_ERROR);
            }
        },
        None => PageRange::default(),
    };

    let mut passwords: Vec<String> = multi_values(m, "password");
    if passwords.is_empty() {
        passwords.push(String::new());
    }

    Config {
        recursion,
        count,
        pagecount,
        quiet: m.get_flag("quiet"),
        passwords,
        max_count: m.get_one::<usize>("max-count").copied().unwrap_or(0),
        debug: m.get_flag("debug"),
        warn_empty: m.get_flag("warn-empty"),
        use_cache: m.get_flag("cache"),
        out,
        excludes,
        includes,
        page_range,
        only_filenames,
        files,
        engine,
    }
}

fn build_engine(
    patterns: &[String],
    ignore_case: bool,
    fixed: bool,
    perl: bool,
) -> SharedEngine {
    let mut engines: Vec<SharedEngine> = Vec::with_capacity(patterns.len());
    for p in patterns {
        let built: Result<SharedEngine, String> = if perl {
            PcreEngine::new(p, ignore_case).map(|e| -> SharedEngine { std::sync::Arc::new(e) })
        } else if fixed {
            FixedEngine::new(p, ignore_case).map(|e| -> SharedEngine { std::sync::Arc::new(e) })
        } else {
            RegexEngineImpl::new(p, ignore_case)
                .map(|e| -> SharedEngine { std::sync::Arc::new(e) })
        };
        match built {
            Ok(e) => engines.push(e),
            Err(msg) => {
                eprint(&msg);
                std::process::exit(EXIT_ERROR);
            }
        }
    }

    if engines.is_empty() {
        // -e/-f given but no patterns: matches nothing.
        return std::sync::Arc::new(NeverEngine);
    }
    if engines.len() == 1 {
        return engines.pop().unwrap();
    }
    std::sync::Arc::new(PatternList::new(engines))
}

/// Collect all values of an Append-style option.
fn multi_values(m: &clap::ArgMatches, id: &str) -> Vec<String> {
    m.get_many::<String>(id)
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default()
}

fn parse_max_count(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!("Could not parse number: {s}.")),
    }
}

fn parse_ctx_num(s: &str) -> Result<i32, String> {
    match s.parse::<i32>() {
        Ok(n) if n >= 0 => Ok(n),
        _ => Err(format!("Could not parse number: {s}.")),
    }
}

/// Parse GREP_COLORS like pdfgrep (`name=value` pairs, `:`-separated).
fn read_grep_colors(colors: &mut ColorScheme) {
    let Ok(val) = std::env::var("GREP_COLORS") else {
        return;
    };
    for pair in val.split(':') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        match name {
            "mt" | "ms" | "mc" => colors.highlight = sgr_value(value),
            "fn" => colors.filename = sgr_value(value),
            "ln" => colors.pagenum = sgr_value(value),
            "se" => colors.separator = sgr_value(value),
            _ => {}
        }
    }
}

fn sgr_value(code: &str) -> String {
    if code.is_empty() {
        String::new()
    } else {
        sgr(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_colors_match_grep() {
        let c = default_colors();
        assert_eq!(c.highlight, "\x1b[01;31m\x1b[K");
        assert_eq!(c.filename, "\x1b[35m\x1b[K");
        assert_eq!(c.pagenum, "\x1b[32m\x1b[K");
        assert_eq!(c.separator, "\x1b[36m\x1b[K");
    }

    #[test]
    fn cli_accepts_dash_e_with_hyphen_pattern() {
        let cmd = build_cli();
        let m = cmd
            .try_get_matches_from(["pdfq", "-e", "-a", "file.pdf"])
            .unwrap();
        let e: Vec<String> = multi_values(&m, "regexp");
        assert_eq!(e, vec!["-a"]);
    }
}
