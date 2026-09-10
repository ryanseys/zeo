//! Can zeo compile this gem? One gem, one row in a TSV.
//!
//! Built to be pointed at a very large corpus of unpacked gems, so the two
//! things that matter are that a run writes almost nothing and that one bad
//! gem cannot wedge the sweep.
//!
//! **In memory by default.** The JIT road compiles the gem's require graph
//! and runs it in the child's own address space with the program cache off,
//! so the only file the tool creates is the TSV row. `--aot` links a real
//! binary, which is the road that writes, and its scratch directory is
//! removed on the way out unless `--no-clean` says to keep it.
//!
//! Every child is bounded. A compiler bug that loops is a FINDING for the
//! row rather than a hang for the sweep.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{Error, root_join};

const USAGE: &str = "\
usage: cargo xtask test-gem <gem>... [options]

Compiles each gem and appends a row saying whether zeo could.

  --path <dir>       compile this directory as the gem (name from its basename)
  --gems-root <dir>  where <gem> is looked up, as <dir>/<gem> or
                     <dir>/<gem>-<version>. Required unless --path is given
  --aot              link a real binary instead of running on the JIT
  --no-clean         keep what --aot wrote, for debugging
  --out <path>       the TSV to append to. Default: target/test-gem.tsv
  --timeout <secs>   bound on one gem. Default: 60

The JIT road writes nothing at all: the program cache is off and the compile
happens in the child's own memory. Only --aot writes, and only into a scratch
directory this removes.

Exits nonzero when any gem failed, so a handful of gems reads as a test. The
TSV is the answer for a sweep, where the exit status means little.
";

const DEFAULT_TIMEOUT: u64 = 60;
const TSV_HEADER: &str = "gem\tversion\troad\tstatus\tkind\tms\tfeature\tdetail\n";
/// Enough of the compiler's complaint to group rows by; the whole thing can
/// be megabytes of backtrace.
const DETAIL_CAP: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Road {
    Jit,
    Aot,
}

impl Road {
    fn name(self) -> &'static str {
        match self {
            Road::Jit => "jit",
            Road::Aot => "aot",
        }
    }
}

/// What happened to one gem. `Missing` is not a zeo verdict at all, which is
/// why it is not a failure: it says the corpus does not have the gem.
enum Status {
    Ok,
    Failed(String),
    TimedOut,
    Missing,
}

impl Status {
    fn word(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Failed(_) => "fail",
            Status::TimedOut => "timeout",
            Status::Missing => "missing",
        }
    }
}

struct Options {
    road: Road,
    clean: bool,
    out: PathBuf,
    timeout: Duration,
    gems_root: Option<PathBuf>,
    path: Option<PathBuf>,
}

pub fn run(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let (gems, opts) = parse(args)?;
    if gems.is_empty() && opts.path.is_none() {
        return Err(Error::new(USAGE.to_string()));
    }
    if opts.path.is_none() && opts.gems_root.is_none() {
        return Err(Error::new(format!(
            "--gems-root <dir> is required to look a gem up by name\n\n{USAGE}"
        )));
    }
    let zeo = crate::build_zeo()?;

    let mut rows = Vec::new();
    for gem in &gems {
        let dir = match (&opts.path, &opts.gems_root) {
            (Some(p), _) => Some(p.clone()),
            (None, Some(root)) => locate(gem, root),
            (None, None) => None,
        };
        let started = Instant::now();
        let (status, feature, kind, version) = match &dir {
            None => (Status::Missing, String::new(), "", String::new()),
            Some(dir) => {
                let spec = spec_of(dir);
                let feature = feature_of(&spec.require_paths[0], gem);
                let status = compile(&zeo, dir, &spec, &feature, &opts)?;
                let kind = match &status {
                    Status::Failed(d) => classify(d, &spec, &feature),
                    _ => "",
                };
                (status, feature, kind, spec.version)
            }
        };
        let ms = started.elapsed().as_millis();
        println!(
            "{:<8} {:<12} {:>7}ms  {gem}{}",
            status.word(),
            kind,
            ms,
            match &status {
                Status::Failed(d) => format!("  -- {}", first_line(d)),
                Status::Missing => "  -- not found in the gem corpus".into(),
                _ => String::new(),
            }
        );
        rows.push(format!(
            "{gem}\t{version}\t{}\t{}\t{kind}\t{ms}\t{feature}\t{}\n",
            opts.road.name(),
            status.word(),
            match &status {
                Status::Failed(d) => tsv_escape(d),
                _ => String::new(),
            }
        ));
    }

    append(&opts.out, &rows)?;
    let bad = rows
        .iter()
        .filter(|r| {
            let word = r.split('\t').nth(3).unwrap_or("");
            word == "fail" || word == "timeout"
        })
        .count();
    println!(
        "\n{} gem(s), {} failed -- {}",
        rows.len(),
        bad,
        opts.out.display()
    );
    if bad > 0 {
        return Err(Error::reported());
    }
    Ok(())
}

fn parse(args: &[String]) -> Result<(Vec<String>, Options), Error> {
    let mut gems = Vec::new();
    let mut opts = Options {
        road: Road::Jit,
        clean: true,
        out: root_join("target/test-gem.tsv"),
        timeout: Duration::from_secs(DEFAULT_TIMEOUT),
        gems_root: None,
        path: None,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |what: &str| -> Result<String, Error> {
            rest.next()
                .cloned()
                .ok_or_else(|| Error::new(format!("{what} needs a value\n\n{USAGE}")))
        };
        match arg.as_str() {
            "--aot" => opts.road = Road::Aot,
            "--no-clean" => opts.clean = false,
            "--out" => opts.out = PathBuf::from(value("--out")?),
            "--path" => opts.path = Some(PathBuf::from(value("--path")?)),
            "--gems-root" => opts.gems_root = Some(PathBuf::from(value("--gems-root")?)),
            "--timeout" => {
                let secs = value("--timeout")?
                    .parse()
                    .map_err(|_| Error::new(format!("--timeout wants seconds\n\n{USAGE}")))?;
                opts.timeout = Duration::from_secs(secs);
            }
            other if other.starts_with('-') => {
                return Err(Error::new(format!("unexpected {other:?}\n\n{USAGE}")));
            }
            name => gems.push(name.to_string()),
        }
    }
    if let (Some(path), true) = (&opts.path, gems.is_empty()) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "gem".into());
        gems.push(name);
    }
    Ok((gems, opts))
}

/// What the gem corpus records beside each gem: where its code is, and
/// whether it ships C. Absent for a plain source tree, where `lib` is the
/// only sensible guess.
struct Spec {
    require_paths: Vec<PathBuf>,
    has_extensions: bool,
    version: String,
}

fn spec_of(dir: &Path) -> Spec {
    let version = std::fs::read_to_string(dir.join(".zeo-probe-version"))
        .map(|v| v.trim().to_string())
        .ok()
        .or_else(|| version_of(dir))
        .unwrap_or_default();
    let json: Option<serde_json::Value> = std::fs::read_to_string(dir.join(".zeo-probe-spec"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let require_paths = json
        .as_ref()
        .and_then(|j| j.get("require_paths"))
        .and_then(|p| p.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .map(|p| dir.join(p))
                .collect::<Vec<_>>()
        })
        .filter(|p: &Vec<PathBuf>| !p.is_empty())
        .unwrap_or_else(|| vec![dir.join("lib")]);
    let has_extensions = json
        .as_ref()
        .and_then(|j| j.get("extensions"))
        .and_then(|e| e.as_array())
        .is_some_and(|a| !a.is_empty());
    Spec {
        require_paths,
        has_extensions,
        version,
    }
}

/// Why a compile failed, so a sweep can tell zeo's own bugs from a gem whose
/// dependencies are simply not on the load path. A heuristic, and named as
/// one: the column groups rows, it does not adjudicate them.
fn classify(detail: &str, spec: &Spec, feature: &str) -> &'static str {
    if detail.contains("panicked at") {
        return "zeo-panic";
    }
    let Some(missing) = detail
        .split("cannot load such file -- ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return "zeo";
    };
    let own = missing.split('/').next().unwrap_or(missing);
    let mine = feature.split('/').next().unwrap_or(feature);
    if own != mine {
        return "missing-dep";
    }
    if spec.has_extensions {
        "cext"
    } else {
        "self-load"
    }
}

/// The gem's unpacked source tree under `root`. An unversioned corpus holds
/// `<root>/<name>`; a gem store holds `<name>-<version>`, and the newest
/// wins there.
fn locate(gem: &str, root: &Path) -> Option<PathBuf> {
    let flat = root.join(gem);
    if flat.is_dir() {
        return Some(flat);
    }
    // `<name>-<version>`: scan only when the flat name missed, and only
    // this one directory. A large corpus is never walked.
    let mut best: Option<PathBuf> = None;
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(rest) = name.strip_prefix(gem)
            && rest.starts_with('-')
            && rest[1..].starts_with(|c: char| c.is_ascii_digit())
            && entry.path().is_dir()
            && best.as_ref().is_none_or(|b: &PathBuf| {
                b.file_name().unwrap_or_default().to_string_lossy() < name
            })
        {
            best = Some(entry.path());
        }
    }
    best
}

fn version_of(dir: &Path) -> Option<String> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let (_, version) = name.rsplit_once('-')?;
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_string())
}

/// What to `require`. A gem's entry file is usually its own name, sometimes
/// its name with the dashes as directories (`io-console` -> `io/console`),
/// and occasionally the only `.rb` at the top of `lib/`.
fn feature_of(lib: &Path, gem: &str) -> String {
    let slashed = gem.replace('-', "/");
    for candidate in [gem, slashed.as_str()] {
        if lib.join(format!("{candidate}.rb")).is_file() {
            return candidate.to_string();
        }
    }
    let mut only = None;
    for entry in std::fs::read_dir(lib).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rb") {
            if only.is_some() {
                return gem.to_string();
            }
            only = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        }
    }
    only.unwrap_or_else(|| gem.to_string())
}

fn compile(
    zeo: &Path,
    dir: &Path,
    spec: &Spec,
    feature: &str,
    opts: &Options,
) -> Result<Status, Error> {
    let program = format!("require {feature:?}\n");
    let mut cmd = Command::new(zeo);

    // The AOT road needs a file and an output path; the JIT road needs
    // neither, and writing neither is the point of it being the default.
    // `build` is a VERB and has to be the first argument, so the two roads
    // do not share a prefix.
    let scratch = match opts.road {
        Road::Jit => {
            for path in &spec.require_paths {
                cmd.arg("-I").arg(path);
            }
            cmd.arg("-e").arg(&program);
            None
        }
        Road::Aot => {
            let scratch = root_join("target/test-gem").join(format!(
                "{}-{}",
                dir.file_name().unwrap_or_default().to_string_lossy(),
                std::process::id()
            ));
            std::fs::create_dir_all(&scratch)
                .map_err(|e| Error::new(format!("{}: {e}", scratch.display())))?;
            let rb = scratch.join("probe.rb");
            std::fs::write(&rb, &program)
                .map_err(|e| Error::new(format!("{}: {e}", rb.display())))?;
            cmd.arg("build").arg(&rb);
            for path in &spec.require_paths {
                cmd.arg("-I").arg(path);
            }
            cmd.arg("-o").arg(scratch.join("probe"));
            Some(scratch)
        }
    };
    // Nothing this runs may reach the developer's caches or a ruby on PATH.
    cmd.env("ZEO_CACHE", "0")
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .env_remove("GEM_HOME")
        .env_remove("GEM_PATH")
        .env_remove("BUNDLE_GEMFILE");

    let status = bounded(cmd, opts.timeout);

    if let Some(scratch) = scratch {
        if opts.clean {
            let _ = std::fs::remove_dir_all(&scratch);
        } else {
            println!("  kept {}", scratch.display());
        }
    }
    Ok(status)
}

/// Run `cmd` to completion or to the deadline, whichever comes first.
fn bounded(mut cmd: Command, deadline: Duration) -> Status {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Status::Failed(format!("spawning zeo: {e}")),
    };
    // stderr is drained on its own thread: a compiler that fills the pipe
    // would otherwise block forever and read as a timeout.
    let mut pipe = child.stderr.take().expect("piped stderr");
    let drain = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(e) => return Status::Failed(format!("waiting on zeo: {e}")),
            Ok(None) => {}
        }
        if started.elapsed() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stderr = drain.join().unwrap_or_default();
    match status {
        None => Status::TimedOut,
        Some(status) if status.success() => Status::Ok,
        Some(status) => {
            let text = String::from_utf8_lossy(&stderr);
            let text = text.trim();
            Status::Failed(if text.is_empty() {
                format!("exit {status}, and it said nothing")
            } else {
                text.to_string()
            })
        }
    }
}

fn append(out: &Path, rows: &[String]) -> Result<(), Error> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::new(format!("{}: {e}", parent.display())))?;
    }
    let fresh = !out.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
        .map_err(|e| Error::new(format!("{}: {e}", out.display())))?;
    if fresh {
        file.write_all(TSV_HEADER.as_bytes())
            .map_err(|e| Error::new(format!("{}: {e}", out.display())))?;
    }
    for row in rows {
        file.write_all(row.as_bytes())
            .map_err(|e| Error::new(format!("{}: {e}", out.display())))?;
    }
    Ok(())
}

/// One row is one line, so the separators cannot survive in a field.
fn tsv_escape(detail: &str) -> String {
    let mut out = String::new();
    for ch in detail.chars() {
        if out.len() >= DETAIL_CAP {
            out.push('…');
            break;
        }
        match ch {
            '\t' => out.push(' '),
            '\n' | '\r' => out.push_str(" | "),
            c => out.push(c),
        }
    }
    out
}

fn first_line(detail: &str) -> &str {
    detail.lines().next().unwrap_or(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_or_a_newline_cannot_break_the_row_it_is_in() {
        let escaped = tsv_escape("one\ttwo\nthree");
        assert!(!escaped.contains('\t'));
        assert!(!escaped.contains('\n'));
        assert_eq!(escaped, "one two | three");
    }

    #[test]
    fn a_long_complaint_is_cut_rather_than_carried_whole() {
        let escaped = tsv_escape(&"x".repeat(DETAIL_CAP * 3));
        assert!(escaped.ends_with('…'));
        assert!(escaped.chars().count() <= DETAIL_CAP + 1);
    }

    #[test]
    fn a_version_is_read_off_the_directory_only_when_it_is_one() {
        assert_eq!(
            version_of(Path::new("/x/csv-3.3.6")).as_deref(),
            Some("3.3.6")
        );
        // The big corpus is unversioned, and `io-console` is not a version.
        assert_eq!(version_of(Path::new("/x/io-console")), None);
        assert_eq!(version_of(Path::new("/x/rake")), None);
    }

    #[test]
    fn the_jit_road_is_the_default_and_aot_is_asked_for() {
        let Ok((gems, opts)) = parse(&["rake".to_string()]) else {
            panic!("a bare gem name parses")
        };
        assert_eq!(gems, ["rake"]);
        assert!(opts.road == Road::Jit && opts.clean);
        let Ok((_, opts)) = parse(&["rake".into(), "--aot".into(), "--no-clean".into()]) else {
            panic!("the flags parse")
        };
        assert!(opts.road == Road::Aot && !opts.clean);
    }
}
