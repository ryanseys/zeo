//! Compiling and linking a C extension, without `make`.
//!
//! mkmf wrote a Makefile; [`super::makefile`] read its variables. What is
//! left is two commands, and they are the two mkmf's own rules spell:
//!
//! ```text
//! .c.o:          $(CC) $(INCFLAGS) $(CPPFLAGS) $(CFLAGS) -o $@ -c $<
//! $(TARGET_SO):  $(LDSHARED) -o $@ $(OBJS) $(LIBPATH) $(DLDFLAGS) \
//!                $(LOCAL_LIBS) $(LIBS)
//! ```
//!
//! # Why not just run `make`
//!
//! Two reasons, and only the second is about speed. A gem's build has to work
//! on a machine with no `make` -- which is a real state on Windows and an
//! increasingly common one in a container. And the compile of `n` sources is
//! embarrassingly parallel, which `make -j` gets right only when the caller
//! remembers to ask.
//!
//! # Why `make` is still here
//!
//! A Makefile carrying a rule mkmf did not write is a Makefile zeo cannot
//! reproduce: a `depend` file, a generated header, a code generator the gem
//! runs first. Driving that ourselves would silently skip the rule.
//! [`Plan::of`] refuses such a Makefile and the caller runs real `make`, so a
//! custom rule is slower and never wrong.
//!
//! `ZEO_CEXT_MAKE=1` forces that path for everything, which is the escape
//! hatch when a build goes wrong and the question is whether zeo's driver is
//! why.

use super::makefile::Makefile;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What to run, in order.
pub struct Plan {
    /// The directory every command runs in. mkmf's `srcdir` is relative to
    /// it, and so is every path in `OBJS`.
    pub dir: PathBuf,
    pub compiles: Vec<Compile>,
    pub link: Link,
    /// The shared object the link produces, relative to `dir`.
    pub product: PathBuf,
}

pub struct Compile {
    pub program: String,
    pub args: Vec<String>,
    pub source: PathBuf,
    pub object: PathBuf,
}

pub struct Link {
    pub program: String,
    pub args: Vec<String>,
}

/// Why a Makefile has to go to real `make`.
#[derive(Debug)]
pub enum Refusal {
    /// `ZEO_CEXT_MAKE=1`.
    Forced,
    /// A rule mkmf did not write, named so the reason is actionable.
    CustomRules(Vec<String>),
    /// A source whose extension has no compile rule here.
    UnknownSource(String),
    /// mkmf wrote no `TARGET`, which means `create_makefile` never ran.
    NoTarget,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Forced => write!(f, "ZEO_CEXT_MAKE=1"),
            Refusal::CustomRules(rules) => {
                write!(
                    f,
                    "the Makefile has rules mkmf did not write: {}",
                    rules.join(", ")
                )
            }
            Refusal::UnknownSource(s) => write!(f, "{s} has no compile rule zeo knows"),
            Refusal::NoTarget => write!(f, "the Makefile declares no TARGET"),
        }
    }
}

/// The compiler for a source, by its extension. C++ and Objective-C++ go
/// through `$(CXX)` and take `$(CXXFLAGS)` -- using the C pair would compile
/// a `.cpp` as C and fail on the first `class`.
fn toolchain(source: &str) -> Option<(&'static str, &'static str)> {
    let ext = Path::new(source).extension()?.to_str()?;
    Some(match ext {
        "c" | "m" => ("CC", "CFLAGS"),
        "cc" | "cpp" | "cxx" | "C" | "mm" => ("CXX", "CXXFLAGS"),
        _ => return None,
    })
}

impl Plan {
    /// Read `dir/Makefile` and decide what to run.
    pub fn of(dir: &Path) -> Result<Result<Plan, Refusal>, std::io::Error> {
        if std::env::var_os("ZEO_CEXT_MAKE").is_some_and(|v| v != "0") {
            return Ok(Err(Refusal::Forced));
        }
        let text = std::fs::read_to_string(dir.join("Makefile"))?;
        Ok(Self::from_makefile(dir, &Makefile::parse(&text)))
    }

    /// The same decision over an already-parsed Makefile, so a test can make
    /// one without writing a file.
    pub fn from_makefile(dir: &Path, mk: &Makefile) -> Result<Plan, Refusal> {
        if !mk.is_stock() {
            let extra = mk.extra_rules().iter().map(|s| (*s).to_string()).collect();
            return Err(Refusal::CustomRules(extra));
        }
        let target_so = mk.get("TARGET_SO");
        if target_so.trim().is_empty() {
            return Err(Refusal::NoTarget);
        }

        // `srcdir` is where the sources are; every other path is relative to
        // the build directory. mkmf sets it to `.` for an in-tree build and
        // to the gem's `ext/` directory for an out-of-tree one.
        let srcdir = match mk.get("srcdir") {
            s if s.trim().is_empty() => ".".to_string(),
            s => s,
        };
        let sources = mk.words("SRCS");
        let objects = mk.words("OBJS");
        let common: Vec<String> = ["INCFLAGS", "CPPFLAGS"]
            .iter()
            .flat_map(|k| mk.words(k))
            .collect();

        let mut compiles = Vec::with_capacity(sources.len());
        for (i, source) in sources.iter().enumerate() {
            let Some((cc, flags)) = toolchain(source) else {
                return Err(Refusal::UnknownSource(source.clone()));
            };
            // mkmf lists `OBJS` in the same order as `SRCS`. When it does
            // not -- a gem that set `$objs` by hand -- the object name is
            // derived, which is what the suffix rule would have produced.
            let object = objects
                .get(i)
                .cloned()
                .unwrap_or_else(|| object_for(source));
            let mut args = common.clone();
            args.extend(mk.words(flags));
            args.push("-o".into());
            args.push(object.clone());
            args.push("-c".into());
            args.push(join(&srcdir, source));
            compiles.push(Compile {
                program: nonempty(mk.get("CC_WRAPPER"), mk.get(cc), "cc"),
                args,
                source: PathBuf::from(join(&srcdir, source)),
                object: PathBuf::from(object),
            });
        }

        let mut args = vec!["-o".to_string(), target_so.clone()];
        args.extend(objects.iter().cloned());
        for key in ["LIBPATH", "DLDFLAGS", "LOCAL_LIBS", "LIBS"] {
            args.extend(mk.words(key));
        }
        // `LDSHARED` carries its own arguments (`clang -dynamic -bundle`), so
        // the first word is the program and the rest lead the line.
        let ldshared = mk.words("LDSHARED");
        let (program, lead) = ldshared
            .split_first()
            .map_or(("cc".to_string(), &[][..]), |(p, r)| (p.clone(), r));
        let mut all = lead.to_vec();
        all.extend(args);

        Ok(Plan {
            dir: dir.to_path_buf(),
            compiles,
            link: Link { program, args: all },
            product: PathBuf::from(target_so.trim()),
        })
    }

    /// Compile every source, then link. The compiles run in parallel; the
    /// link waits for all of them, which is the only ordering there is.
    ///
    /// A failing compile stops the others from STARTING but does not
    /// interrupt one already running: a half-written object file is worse
    /// than a wasted second.
    pub fn run(&self, jobs: usize) -> Result<PathBuf, Failure> {
        let width = jobs.max(1).min(self.compiles.len().max(1));
        let next = std::sync::atomic::AtomicUsize::new(0);
        let failed = std::sync::Mutex::new(None::<Failure>);

        std::thread::scope(|scope| {
            for _ in 0..width {
                scope.spawn(|| {
                    loop {
                        if failed.lock().is_ok_and(|f| f.is_some()) {
                            return;
                        }
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(job) = self.compiles.get(i) else {
                            return;
                        };
                        if let Err(e) = self.one(&job.program, &job.args, &job.source) {
                            let mut slot = failed.lock().expect("a poisoned lock needs a panic");
                            slot.get_or_insert(e);
                            return;
                        }
                    }
                });
            }
        });
        if let Some(e) = failed.into_inner().expect("the scope has joined") {
            return Err(e);
        }

        self.one(&self.link.program, &self.link.args, &self.product)?;
        Ok(self.dir.join(&self.product))
    }

    fn one(&self, program: &str, args: &[String], what: &Path) -> Result<(), Failure> {
        let out = Command::new(program)
            .args(args)
            .current_dir(&self.dir)
            .output()
            .map_err(|e| Failure {
                what: what.display().to_string(),
                command: rendered(program, args),
                message: e.to_string(),
            })?;
        if out.status.success() {
            return Ok(());
        }
        // The compiler's own diagnostics are the whole value of the failure,
        // so both streams travel rather than an exit code.
        let mut message = String::from_utf8_lossy(&out.stderr).into_owned();
        message.push_str(&String::from_utf8_lossy(&out.stdout));
        Err(Failure {
            what: what.display().to_string(),
            command: rendered(program, args),
            message,
        })
    }
}

/// A compile or link that failed, with the command and the diagnostics.
#[derive(Debug)]
pub struct Failure {
    pub what: String,
    pub command: String,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "building {} failed\n  {}\n{}",
            self.what,
            self.command,
            self.message.trim_end()
        )
    }
}

/// Run real `make`, for a Makefile [`Plan::of`] refused.
pub fn run_make(dir: &Path, jobs: usize) -> Result<(), Failure> {
    let out = Command::new("make")
        .arg(format!("-j{}", jobs.max(1)))
        .current_dir(dir)
        .output()
        .map_err(|e| Failure {
            what: "make".into(),
            command: "make".into(),
            message: e.to_string(),
        })?;
    if out.status.success() {
        return Ok(());
    }
    let mut message = String::from_utf8_lossy(&out.stderr).into_owned();
    message.push_str(&String::from_utf8_lossy(&out.stdout));
    Err(Failure {
        what: "make".into(),
        command: "make".into(),
        message,
    })
}

fn rendered(program: &str, args: &[String]) -> String {
    let mut out = program.to_string();
    for a in args {
        out.push(' ');
        out.push_str(a);
    }
    out
}

/// `$(CC_WRAPPER) $(CC)`, falling back to `cc`. A wrapper is how ccache and
/// the cross toolchains get in front of the compiler, and mkmf leaves it
/// empty when there is none.
fn nonempty(wrapper: String, cc: String, fallback: &str) -> String {
    let cc = cc.trim();
    let base = if cc.is_empty() { fallback } else { cc };
    let wrapper = wrapper.trim();
    if wrapper.is_empty() {
        // `CC` may itself carry arguments; only the first word is the
        // program, and the rest are already in `CFLAGS` for every mkmf
        // Makefile in the census.
        return base
            .split_whitespace()
            .next()
            .unwrap_or(fallback)
            .to_string();
    }
    wrapper
        .split_whitespace()
        .next()
        .unwrap_or(fallback)
        .to_string()
}

/// `foo/bar.c` compiles to `bar.o`, in the build directory rather than
/// beside the source. That is what mkmf's suffix rule produces, because
/// `make` resolves `$@` against the target list.
fn object_for(source: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    format!("{stem}.o")
}

fn join(dir: &str, name: &str) -> String {
    if dir == "." || dir.is_empty() || name.starts_with('/') {
        return name.to_string();
    }
    format!("{}/{name}", dir.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Makefile of the shape mkmf writes, cut down to what the plan reads.
    fn stock(extra: &str) -> Makefile {
        Makefile::parse(&format!(
            "\
srcdir = .
CC = cc
CXX = c++
CC_WRAPPER =
INCFLAGS = -I. -I/hdr
CPPFLAGS =
CFLAGS = -fPIC -O2 $(ARCH_FLAG)
CXXFLAGS = -fPIC -O2 -std=c++17
ARCH_FLAG =
LDSHARED = clang -dynamic -bundle
LIBPATH = -L.
DLDFLAGS = -Wl,-undefined,dynamic_lookup
LOCAL_LIBS =
LIBS = -lm -lpthread
ORIG_SRCS = a.c b.c
SRCS = $(ORIG_SRCS)
OBJS = a.o b.o
TARGET = probe
DLLIB = $(TARGET).bundle
TARGET_SO = $(DLLIB)

all: $(DLLIB)

$(TARGET_SO): $(OBJS) Makefile
\t$(LDSHARED) -o $@ $(OBJS)

.c.o:
\t$(CC) -c $<
{extra}"
        ))
    }

    #[test]
    fn the_compile_line_is_the_suffix_rules_own() {
        let plan = Plan::from_makefile(Path::new("/tmp/x"), &stock(""))
            .unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(plan.compiles.len(), 2);
        let first = &plan.compiles[0];
        assert_eq!(first.program, "cc");
        assert_eq!(
            first.args,
            ["-I.", "-I/hdr", "-fPIC", "-O2", "-o", "a.o", "-c", "a.c"]
        );
        assert_eq!(plan.compiles[1].object, Path::new("b.o"));
    }

    /// `LDSHARED` carries its own arguments, so the first word is the
    /// program and the rest have to LEAD the line -- putting them after
    /// `-o` would hand `clang` a `-dynamic` it reads as a link input.
    #[test]
    fn the_link_line_keeps_ldshareds_own_arguments_in_front() {
        let plan = Plan::from_makefile(Path::new("/tmp/x"), &stock(""))
            .unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(plan.link.program, "clang");
        assert_eq!(
            plan.link.args,
            [
                "-dynamic",
                "-bundle",
                "-o",
                "probe.bundle",
                "a.o",
                "b.o",
                "-L.",
                "-Wl,-undefined,dynamic_lookup",
                "-lm",
                "-lpthread"
            ]
        );
        assert_eq!(plan.product, Path::new("probe.bundle"));
    }

    /// A rule mkmf did not write is the whole reason `make` is still here.
    /// Driving such a Makefile would skip it silently.
    #[test]
    fn an_extra_rule_sends_the_build_to_make() {
        let mk = stock("generated.h: gen.rb\n\truby gen.rb\n");
        match Plan::from_makefile(Path::new("/tmp/x"), &mk) {
            Err(Refusal::CustomRules(rules)) => assert_eq!(rules, ["generated.h"]),
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
    }

    /// A C++ source takes `$(CXX)` and `$(CXXFLAGS)`. Compiling it as C
    /// fails on the first `class`, and the flags differ too.
    #[test]
    fn a_cpp_source_uses_the_cxx_toolchain() {
        let mk = Makefile::parse(
            "srcdir = .\nCC = cc\nCXX = c++\nCFLAGS = -fPIC\nCXXFLAGS = -fPIC -std=c++17\n\
             INCFLAGS = -I.\nSRCS = a.cpp\nOBJS = a.o\nTARGET = x\nTARGET_SO = x.bundle\n\
             LDSHARED = clang -bundle\n",
        );
        let plan = Plan::from_makefile(Path::new("/tmp/x"), &mk)
            .unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(plan.compiles[0].program, "c++");
        assert!(plan.compiles[0].args.contains(&"-std=c++17".to_string()));
    }

    #[test]
    fn a_source_with_no_rule_is_refused_by_name() {
        let mk = Makefile::parse(
            "srcdir = .\nCC = cc\nSRCS = a.rs\nOBJS = a.o\nTARGET = x\nTARGET_SO = x.bundle\n",
        );
        match Plan::from_makefile(Path::new("/tmp/x"), &mk) {
            Err(Refusal::UnknownSource(s)) => assert_eq!(s, "a.rs"),
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
    }

    /// An out-of-tree build sets `srcdir` and the sources are under it,
    /// while the objects stay in the build directory.
    #[test]
    fn an_out_of_tree_srcdir_reaches_the_sources_and_not_the_objects() {
        let mk = Makefile::parse(
            "srcdir = /gem/ext/foo\nCC = cc\nCFLAGS =\nINCFLAGS =\nSRCS = a.c\nOBJS = a.o\n\
             TARGET = foo\nTARGET_SO = foo.bundle\nLDSHARED = cc -bundle\n",
        );
        let plan = Plan::from_makefile(Path::new("/build"), &mk)
            .unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(plan.compiles[0].source, Path::new("/gem/ext/foo/a.c"));
        assert_eq!(plan.compiles[0].object, Path::new("a.o"));
        assert!(plan.compiles[0].args.ends_with(&[
            "-o".to_string(),
            "a.o".to_string(),
            "-c".to_string(),
            "/gem/ext/foo/a.c".to_string()
        ]));
    }

    /// `create_makefile` never ran, so there is nothing to build. Saying so
    /// beats linking an empty bundle.
    #[test]
    fn a_makefile_with_no_target_is_refused() {
        let mk = Makefile::parse("CC = cc\n");
        assert!(matches!(
            Plan::from_makefile(Path::new("/tmp/x"), &mk),
            Err(Refusal::NoTarget)
        ));
    }
}
