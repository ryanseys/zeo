//! The corpus suites: one row per directory under `test/`, stating what a
//! program there is held to. The harness and `cargo xtask bless` both read
//! this table (bless through `#[path]`), so a directory's contract is written
//! once.
//!
//! A program's contract is its DIRECTORY, never a per-file marker: a
//! directory decides who records the answer, whether zeo must match it, and
//! how deep the tests sit. Anything deeper than a suite's depth is a fixture.

use std::path::{Path, PathBuf};

/// Who writes the trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorder {
    /// The pinned ruby: the answer zeo must match.
    Ruby,
    /// zeo itself: a decided divergence, or a program zeo must reject.
    Zeo,
}

/// What a run of the program must do against its trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Match,
    /// A gap: the run must NOT match yet, and a match fails with "promote".
    Xfail,
}

/// How far below the root a program sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// `<root>/<name>.rb`
    One,
    /// `<root>/<area>/<name>.rb`
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Suite {
    /// The case-name prefix (`core::string/upcase.rb`).
    pub name: &'static str,
    /// The directory, relative to the repo root.
    pub root: &'static str,
    pub depth: Depth,
    pub recorder: Recorder,
    pub verdict: Verdict,
    /// Every case splices a whole require graph: minutes and gigabytes, so
    /// the bounds are raised and the zeo side gets the oracle store's rspec
    /// on its load path.
    pub whole_graph: bool,
}

const fn suite(name: &'static str, root: &'static str, depth: Depth) -> Suite {
    Suite {
        name,
        root,
        depth,
        recorder: Recorder::Ruby,
        verdict: Verdict::Match,
        whole_graph: false,
    }
}

pub const LANG: Suite = suite("lang", "test/lang", Depth::Two);
pub const CORE: Suite = suite("core", "test/core", Depth::Two);
pub const STDLIB: Suite = suite("stdlib", "test/stdlib", Depth::Two);
pub const COMPILER: Suite = suite("compiler", "test/compiler", Depth::Two);
/// The curated link tier: these run on the JIT like every other program,
/// and again through a real link.
pub const AOT: Suite = suite("aot", "test/aot", Depth::One);
pub const BENCH: Suite = suite("bench", "test/bench", Depth::One);
pub const ERRORS: Suite = Suite {
    recorder: Recorder::Zeo,
    ..suite("errors", "test/errors", Depth::One)
};
/// Programs that use something only zeo has: embedded sources, `Zeo.prepare`,
/// the cycle collector's own messages, a stack ruby cannot match. Recorded
/// from zeo, because ruby cannot run them.
pub const FEATURES: Suite = Suite {
    recorder: Recorder::Zeo,
    ..suite("features", "test/features", Depth::One)
};
pub const DIVERGENCES: Suite = Suite {
    recorder: Recorder::Zeo,
    ..suite("divergences", "test/divergences", Depth::One)
};
pub const MILESTONES: Suite = Suite {
    whole_graph: true,
    ..suite("milestones", "test/milestones", Depth::One)
};
pub const ZE0: Suite = suite("ze0", "test/ze0", Depth::One);
/// Programs zeo does not get right yet. These run like any other case, but
/// the verdict is inverted: a gap that still differs from ruby is green, and
/// one that MATCHES is red, so the day a gap starts working the suite says
/// so and `cargo xtask promote-gap` moves it to a topic directory.
pub const GAPS: Suite = Suite {
    verdict: Verdict::Xfail,
    ..suite("gaps", "test/gaps", Depth::One)
};

pub const SUITES: &[Suite] = &[
    LANG,
    CORE,
    STDLIB,
    COMPILER,
    AOT,
    BENCH,
    ERRORS,
    FEATURES,
    DIVERGENCES,
    MILESTONES,
    ZE0,
    GAPS,
];

/// The directory every program runs in, relative to the repo root. A
/// backtrace names a program by its path from here.
pub const RUN_CWD: &str = "test";

impl Suite {
    pub fn by_name(name: &str) -> Option<&'static Suite> {
        SUITES.iter().find(|s| s.name == name)
    }

    /// The suite a program under `test/` belongs to, by its directory.
    pub fn of(rel_to_repo: &Path) -> Option<&'static Suite> {
        let mut best: Option<&'static Suite> = None;
        for s in SUITES {
            if rel_to_repo.starts_with(s.root) && best.is_none_or(|b| s.root.len() > b.root.len()) {
                best = Some(s);
            }
        }
        best
    }

    /// Every program under this suite's root, as paths relative to the root,
    /// sorted. Only the suite's depth counts; deeper files are fixtures.
    pub fn cases(&self, repo: &Path) -> std::io::Result<Vec<PathBuf>> {
        let root = repo.join(self.root);
        let mut out = Vec::new();
        match self.depth {
            Depth::One => out.extend(ruby_files(&root, "")?),
            Depth::Two => {
                for area in dirs(&root)? {
                    let prefix = format!(
                        "{}/",
                        area.file_name().unwrap_or_default().to_string_lossy()
                    );
                    out.extend(ruby_files(&area, &prefix)?);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Whether `rel` (relative to the root) is a case at this suite's depth.
    pub fn accepts(&self, rel: &str) -> bool {
        if !rel.ends_with(".rb") {
            return false;
        }
        let depth = rel.matches('/').count();
        match self.depth {
            Depth::One => depth == 0,
            Depth::Two => depth == 1,
        }
    }

    /// The datatest pattern for this suite's depth.
    pub const fn pattern(&self) -> &'static str {
        match self.depth {
            Depth::One => r"^[^/]+\.rb$",
            Depth::Two => r"^[^/]+/[^/]+\.rb$",
        }
    }
}

fn ruby_files(dir: &Path, prefix: &str) -> std::io::Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().ends_with(".rb") && entry.path().is_file() {
            out.push(PathBuf::from(format!("{prefix}{}", name.to_string_lossy())));
        }
    }
    Ok(out)
}

fn dirs(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
