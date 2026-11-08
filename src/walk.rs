//! Recursive directory traversal with include/exclude globs.
//!
//! Entries are sorted per directory so output order is deterministic (pdfgrep
//! uses raw readdir order, which makes tests flaky). Symlink rules follow
//! pdfgrep: `-r` skips every symlink entry, `-R` follows them; visited
//! (dev, ino) pairs break symlink cycles.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};

use crate::cli::Recursion;

pub struct WalkError {
    pub path: String,
    pub reason: String,
}

pub struct Walker {
    includes: Vec<GlobMatcher>,
    excludes: Vec<GlobMatcher>,
    recursion: Recursion,
    /// When false (command-line files), include/exclude do not apply.
    apply_filters: bool,
}

/// fnmatch(FNM_PERIOD) approximation: a leading dot in the name requires a
/// leading dot in the pattern.
fn leading_dot_ok(pattern: &str, name: &str) -> bool {
    !(name.starts_with('.') && !pattern.starts_with('.'))
}

impl Walker {
    pub fn new(
        includes: &[String],
        excludes: &[String],
        recursion: Recursion,
        apply_filters: bool,
    ) -> Self {
        let compile = |p: &str| -> GlobMatcher {
            Glob::new(p)
                .map(|g| g.compile_matcher())
                // An invalid glob simply never matches; pdfgrep's fnmatch
                // would too (fnmatch returns error -> treated as no match).
                .unwrap_or_else(|_| Glob::new("*__never__").unwrap().compile_matcher())
        };
        Self {
            includes: includes.iter().map(|s| compile(s)).collect(),
            excludes: excludes.iter().map(|s| compile(s)).collect(),
            recursion,
            apply_filters,
        }
    }

    pub fn accepts(&self, basename: &str) -> bool {
        if !self.apply_filters {
            return true;
        }
        let included = self.includes.iter().any(|g| {
            leading_dot_ok(g.glob().to_string().as_str(), basename) && g.is_match(basename)
        });
        if !included {
            return false;
        }
        !self.excludes.iter().any(|g| {
            leading_dot_ok(g.glob().to_string().as_str(), basename) && g.is_match(basename)
        })
    }

    /// Expand one command-line argument into concrete PDF files to search.
    /// Non-recursive mode passes files through unchanged (errors surface at
    /// open time, matching pdfgrep).
    pub fn expand_arg(&self, arg: &str, out: &mut Vec<String>, errors: &mut Vec<WalkError>) {
        let path = Path::new(arg);
        let is_dir = path.is_dir(); // follows symlinks, like pdfgrep's stat()

        if !is_dir {
            out.push(arg.to_string());
            return;
        }

        if self.recursion == Recursion::None {
            errors.push(WalkError {
                path: arg.to_string(),
                reason: "is a directory. Did you mean to use '--recursive'?".into(),
            });
            return;
        }

        let mut visited = HashSet::new();
        self.walk_dir(arg, out, errors, &mut visited);
    }

    fn walk_dir(
        &self,
        dir: &str,
        out: &mut Vec<String>,
        errors: &mut Vec<WalkError>,
        visited: &mut HashSet<(u64, u64)>,
    ) {
        // Cycle protection only matters when following symlinks (-R).
        if self.recursion == Recursion::FollowSymlinks
            && let Ok(md) = fs::metadata(dir)
                && !visited.insert((md.dev(), md.ino())) {
                    return;
                }

        let rd = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                errors.push(WalkError {
                    path: dir.to_string(),
                    reason: e.to_string(),
                });
                return;
            }
        };

        let mut entries: Vec<(String, PathBuf)> = Vec::new();
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if name == "." || name == ".." {
                continue;
            }
            entries.push((name, ent.path()));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, path) in entries {
            let display = format!("{dir}/{name}");

            let meta = if self.recursion == Recursion::FollowSymlinks {
                fs::metadata(&path)
            } else {
                fs::symlink_metadata(&path)
            };
            let meta = match meta {
                Ok(m) => m,
                Err(e) => {
                    errors.push(WalkError {
                        path: display,
                        reason: e.to_string(),
                    });
                    continue;
                }
            };

            let ft = meta.file_type();
            if ft.is_symlink() {
                // -r: skip symlinks entirely (pdfgrep checks S_ISLNK after
                // lstat). -R: symlinks were stat'ed above, so they appear
                // as regular files or directories here.
                if self.recursion != Recursion::FollowSymlinks {
                    continue;
                }
                // stat() on a broken symlink fails and landed in Err above;
                // reaching here means a valid target.
                if meta.is_dir() {
                    self.walk_dir(&display, out, errors, visited);
                } else if self.accepts(&name) {
                    out.push(display);
                }
                continue;
            }

            if meta.is_dir() {
                self.walk_dir(&display, out, errors, visited);
            } else if self.accepts(&name) {
                out.push(display);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_default_pdf_glob() {
        let w = Walker::new(&["*.[Pp][Dd][Ff]".into()], &[], Recursion::None, true);
        assert!(w.accepts("a.pdf"));
        assert!(w.accepts("a.PDF"));
        assert!(w.accepts("a.PdF"));
        assert!(!w.accepts("a.txt"));
        assert!(!w.accepts(".hidden.pdf")); // FNM_PERIOD
    }

    #[test]
    fn exclude_takes_precedence() {
        let w = Walker::new(
            &["*.[Pp][Dd][Ff]".into()],
            &["b*".into()],
            Recursion::None,
            true,
        );
        assert!(w.accepts("abc.pdf"));
        assert!(!w.accepts("bca.pdf"));
    }

    #[test]
    fn filters_disabled_for_cli_args() {
        let w = Walker::new(&["*.[Pp][Dd][Ff]".into()], &[], Recursion::None, false);
        assert!(w.accepts("anything.txt"));
    }
}
