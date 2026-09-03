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
    /// An object with no source beside it. A gem that GENERATES one has a
    /// rule for it, and that rule is `make`'s to run.
    NoSource(String),
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
            Refusal::NoSource(o) => write!(f, "no source file beside {o}"),
            Refusal::NoTarget => write!(f, "the Makefile declares no TARGET"),
        }
    }
}

/// The flags that strip the unwind tables a raise travels through. mkmf
/// drops them from a gem's own additions with a warning, so one reaching
/// a Makefile came from a rule zeo cannot see -- and neither road builds it.
pub const UNWIND_OFF: &[&str] = &[
    "-fno-exceptions",
    "-fno-unwind-tables",
    "-fno-asynchronous-unwind-tables",
];

fn unwind_tables_off(mk: &Makefile) -> Option<String> {
    ["CFLAGS", "CXXFLAGS"]
        .iter()
        .flat_map(|k| mk.words(k))
        .find(|w| UNWIND_OFF.contains(&w.as_str()))
}

/// The compiler for a source, by its extension, and which flags it takes.
///
/// C++ and Objective-C++ go through `$(CXX)` and `$(CXXFLAGS)` -- using the C
/// pair would compile a `.cpp` as C and fail on the first `class`. Assembly
/// goes through `$(CC)` too, which is what make's own built-in `.S.o` rule
/// does: the file needs the preprocessor before the assembler.
fn toolchain(ext: &str) -> Option<(&'static str, &'static str)> {
    Some(match ext {
        "c" | "m" | "S" | "s" => ("CC", "CFLAGS"),
        "cc" | "cpp" | "cxx" | "C" | "mm" => ("CXX", "CXXFLAGS"),
        _ => return None,
    })
}

/// The suffixes an object's source may have, in the order make searches
/// `.SUFFIXES`. mkmf writes exactly this list, plus `.S` from make's own
/// built-in rules.
const SOURCE_SUFFIXES: &[&str] = &["c", "m", "cc", "mm", "cxx", "cpp", "C", "S", "s"];

impl Plan {
    /// Read `dir/Makefile` and decide what to run.
    pub fn of(dir: &Path) -> Result<Result<Plan, Refusal>, std::io::Error> {
        if std::env::var_os("ZEO_CEXT_MAKE").is_some_and(|v| v != "0") {
            return Ok(Err(Refusal::Forced));
        }
        let text = std::fs::read_to_string(dir.join("Makefile"))?;
        let mk = Makefile::parse(&text);
        if let Some(flag) = unwind_tables_off(&mk) {
            return Err(std::io::Error::other(format!(
                "the Makefile turns unwind tables off with `{flag}`; a raise is an unwind \
                 through the extension's own frames (docs/how-to/add-an-extension.md)"
            )));
        }
        Ok(Self::from_makefile(dir, &mk))
    }

    /// The same decision over an already-parsed Makefile, so a test can make
    /// one without writing a file.
    ///
    /// The walk is over `OBJS`, not `SRCS`, because that is what `make`
    /// builds -- `SRCS` is a dependency list and the two are not even the
    /// same length. `bcrypt` sets `$objs` by hand to include `x86.o`, whose
    /// source is `x86.S`; mkmf still writes `x86.c` into `SRCS`, and pairing
    /// the two lists positionally asks the compiler for a file that does not
    /// exist.
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
        let objects = mk.words("OBJS");
        let common: Vec<String> = ["INCFLAGS", "CPPFLAGS"]
            .iter()
            .flat_map(|k| mk.words(k))
            .collect();

        let mut compiles = Vec::with_capacity(objects.len());
        for object in &objects {
            let (source, ext) = source_for(dir, &srcdir, object)
                .ok_or_else(|| Refusal::NoSource(object.clone()))?;
            let Some((cc, flags)) = toolchain(&ext) else {
                return Err(Refusal::UnknownSource(source.clone()));
            };
            let mut args = common.clone();
            args.extend(mk.words(flags));
            args.push("-o".into());
            args.push(object.clone());
            args.push("-c".into());
            args.push(source.clone());
            compiles.push(Compile {
                program: nonempty(mk.get("CC_WRAPPER"), mk.get(cc), "cc"),
                args,
                source: PathBuf::from(source),
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

/// The source `make` would build `object` from: the same stem with each
/// suffix in `.SUFFIXES` order, first one that exists.
///
/// Answers the path as the compile line should spell it, plus the extension
/// that decided the toolchain.
fn source_for(dir: &Path, srcdir: &str, object: &str) -> Option<(String, String)> {
    let stem = Path::new(object).file_stem()?.to_str()?;
    for ext in SOURCE_SUFFIXES {
        let name = format!("{stem}.{ext}");
        let rel = join(srcdir, &name);
        let abs = if Path::new(&rel).is_absolute() {
            PathBuf::from(&rel)
        } else {
            dir.join(&rel)
        };
        if abs.is_file() {
            return Some((rel, (*ext).to_string()));
        }
    }
    None
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

    /// A scratch build directory holding `files`, so the suffix search has
    /// something to find. The plan reads the FILESYSTEM now -- which is what
    /// `make` does, and what the `x86.S` case forced.
    struct Dir(PathBuf);

    impl Dir {
        fn with(name: &str, files: &[&str]) -> Dir {
            let dir = std::env::temp_dir().join(format!(
                "zeo-plan-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            for f in files {
                std::fs::write(dir.join(f), "").expect("a scratch source");
            }
            Dir(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A Makefile of the shape mkmf writes, cut down to what the plan reads.
    fn stock(objs: &str, extra: &str) -> Makefile {
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
OBJS = {objs}
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
        let dir = Dir::with("compile", &["a.c", "b.c"]);
        let plan = Plan::from_makefile(&dir.0, &stock("a.o b.o", ""))
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
        let dir = Dir::with("link", &["a.c", "b.c"]);
        let plan = Plan::from_makefile(&dir.0, &stock("a.o b.o", ""))
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

    /// The `bcrypt` case. `$objs` names `x86.o`, whose source is `x86.S`;
    /// mkmf still writes `x86.c` into `SRCS`. Pairing `SRCS` and `OBJS`
    /// positionally asks the compiler for a file that does not exist, which
    /// is exactly what this used to do.
    #[test]
    fn an_object_finds_its_source_by_suffix_not_by_position() {
        let dir = Dir::with("suffix", &["one.c", "x86.S", "two.cpp"]);
        let plan = Plan::from_makefile(&dir.0, &stock("one.o x86.o two.o", ""))
            .unwrap_or_else(|e| panic!("refused: {e}"));
        let sources: Vec<&Path> = plan.compiles.iter().map(|c| c.source.as_path()).collect();
        assert_eq!(
            sources,
            [Path::new("one.c"), Path::new("x86.S"), Path::new("two.cpp")]
        );
        // Assembly goes through `$(CC)`, which is make's own built-in rule.
        assert_eq!(plan.compiles[1].program, "cc");
        // And C++ through `$(CXX)`, with its own flags.
        assert_eq!(plan.compiles[2].program, "c++");
        assert!(plan.compiles[2].args.contains(&"-std=c++17".to_string()));
    }

    /// A rule mkmf did not write is the whole reason `make` is still here.
    /// Driving such a Makefile would skip it silently.
    #[test]
    fn an_extra_rule_sends_the_build_to_make() {
        let dir = Dir::with("extra", &["a.c", "b.c"]);
        let mk = stock("a.o b.o", "generated.h: gen.rb\n\truby gen.rb\n");
        match Plan::from_makefile(&dir.0, &mk) {
            Err(Refusal::CustomRules(rules)) => assert_eq!(rules, ["generated.h"]),
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
    }

    /// An object with no source beside it means the gem GENERATES one, and
    /// the rule that does is `make`'s to run.
    #[test]
    fn an_object_with_no_source_is_refused_by_name() {
        let dir = Dir::with("nosource", &["a.c"]);
        match Plan::from_makefile(&dir.0, &stock("a.o generated.o", "")) {
            Err(Refusal::NoSource(o)) => assert_eq!(o, "generated.o"),
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
    }

    /// An out-of-tree build sets `srcdir` and the sources are under it,
    /// while the objects stay in the build directory.
    #[test]
    fn an_out_of_tree_srcdir_reaches_the_sources_and_not_the_objects() {
        let dir = Dir::with("outoftree", &[]);
        let src = dir.0.join("src");
        std::fs::create_dir_all(&src).expect("a source directory");
        std::fs::write(src.join("a.c"), "").expect("a scratch source");
        let mk = Makefile::parse(&format!(
            "srcdir = {}\nCC = cc\nCFLAGS =\nINCFLAGS =\nOBJS = a.o\n\
             TARGET = foo\nTARGET_SO = foo.bundle\nLDSHARED = cc -bundle\n",
            src.display()
        ));
        let plan = Plan::from_makefile(&dir.0, &mk).unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(plan.compiles[0].source, src.join("a.c"));
        assert_eq!(plan.compiles[0].object, Path::new("a.o"));
    }

    /// `create_makefile` never ran, so there is nothing to build. Saying so
    /// beats linking an empty bundle.
    #[test]
    fn a_makefile_that_turns_unwind_tables_off_is_named() {
        let mk = Makefile::parse("CFLAGS = -O2 -fno-asynchronous-unwind-tables\nCXXFLAGS = -O2\n");
        assert_eq!(
            unwind_tables_off(&mk).as_deref(),
            Some("-fno-asynchronous-unwind-tables")
        );
        let mk = Makefile::parse("CFLAGS = -O2 -fexceptions\nCXXFLAGS = -O2\n");
        assert_eq!(unwind_tables_off(&mk), None);
    }

    #[test]
    fn a_makefile_with_no_target_is_refused() {
        let dir = Dir::with("notarget", &[]);
        let mk = Makefile::parse("CC = cc\n");
        assert!(matches!(
            Plan::from_makefile(&dir.0, &mk),
            Err(Refusal::NoTarget)
        ));
    }
}
