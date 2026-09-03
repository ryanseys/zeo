//! One-off: move `tests/` (sidecar goldens) to `test/` (`__END__` trailers).
//!
//! Reads a mapping of `old path -> new path under test/`, converts every
//! sidecar into a directive or a trailer section, moves each program's
//! fixture directory with it, and reports whatever is left under `tests/`
//! so nothing is lost silently. Deleted once the migration lands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::case::{Answer, Case, Trailer};
use crate::{Error, root_join};

const USAGE: &str = "usage: cargo xtask migrate-corpus <mapping.tsv>";

const SIDECARS: &[&str] = &[
    ".expected",
    ".err.expected",
    ".linux.expected",
    ".linux.err.expected",
    ".args",
    ".stdin",
    ".gc",
    ".leakcheck",
    ".gccheck",
    ".pkggap",
    ".divergence",
];

pub fn run(args: &[String]) -> Result<(), Error> {
    let Some(mapping) = args.first() else {
        return Err(Error::new(USAGE));
    };
    let text =
        std::fs::read_to_string(mapping).map_err(|e| Error::new(format!("{mapping}: {e}")))?;
    let mut rows: Vec<(PathBuf, PathBuf)> = Vec::new();
    for line in text.lines() {
        let Some((old, new)) = line.split_once('\t') else {
            continue;
        };
        rows.push((root_join(old), root_join("test").join(new)));
    }
    let mut moved = 0;
    let mut dropped = 0;
    for (old, new) in &rows {
        if new.file_name().is_some_and(|n| n == "DROP") {
            remove_program(old)?;
            dropped += 1;
            continue;
        }
        if old.is_dir() {
            move_dir(old, new)?;
            continue;
        }
        migrate_one(old, new)?;
        moved += 1;
    }
    eprintln!("migrate-corpus: {moved} programs moved, {dropped} duplicates dropped");
    let mut left = Vec::new();
    leftovers(&root_join("tests"), &mut left)?;
    if !left.is_empty() {
        eprintln!("left under tests/ ({}):", left.len());
        for p in &left {
            eprintln!("  {}", p.display());
        }
    }
    Ok(())
}

fn side(rb: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", rb.display()))
}

fn read_opt(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::new(format!("{}: {e}", path.display()))),
    }
}

fn remove_program(rb: &Path) -> Result<(), Error> {
    for suffix in std::iter::once("").chain(SIDECARS.iter().copied()) {
        let p = side(rb, suffix);
        if p.is_file() {
            std::fs::remove_file(&p).map_err(|e| Error::new(format!("{}: {e}", p.display())))?;
        }
    }
    Ok(())
}

fn move_dir(old: &Path, new: &Path) -> Result<(), Error> {
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::new(format!("{}: {e}", parent.display())))?;
    }
    std::fs::rename(old, new).map_err(|e| {
        Error::new(format!(
            "moving {} to {}: {e}",
            old.display(),
            new.display()
        ))
    })
}

/// The lines a Ruby file must keep first: a shebang and the magic comments.
fn is_magic(line: &[u8]) -> bool {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    text.starts_with("#!")
        || (text.starts_with('#')
            && text.contains(':')
            && [
                "frozen_string_literal",
                "encoding",
                "coding",
                "warn_indent",
                "shareable_constant_value",
                "-*-",
            ]
            .iter()
            .any(|m| text.contains(m)))
}

fn migrate_one(old: &Path, new: &Path) -> Result<(), Error> {
    let source = std::fs::read(old).map_err(|e| Error::new(format!("{}: {e}", old.display())))?;
    let text = String::from_utf8_lossy(&source).into_owned();
    let old_dir = old.parent().unwrap_or(old);
    let new_dir = new.parent().unwrap_or(new);
    let stem = old
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(new_dir)
        .map_err(|e| Error::new(format!("{}: {e}", new_dir.display())))?;
    let old_dir_name = old_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let depth = new
        .strip_prefix(root_join("test"))
        .map(|p| p.components().count() - 1)
        .unwrap_or(0);

    // Directives.
    let mut directives: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    if let Some(args) = read_opt(&side(old, ".args"))? {
        let mut words = Vec::new();
        for w in String::from_utf8_lossy(&args).split_whitespace() {
            // A word naming a file relative to tests/ (the old run cwd)
            // moves beside the program and is renamed relative to test/.
            let candidate = root_join("tests").join(w);
            if candidate.is_file() {
                let name = candidate.file_name().unwrap().to_owned();
                let dest = new_dir.join(&name);
                std::fs::rename(&candidate, &dest)
                    .map_err(|e| Error::new(format!("{}: {e}", candidate.display())))?;
                let rel = dest
                    .strip_prefix(root_join("test"))
                    .unwrap()
                    .display()
                    .to_string();
                words.push(rel);
            } else {
                words.push(w.to_string());
            }
        }
        directives.push(format!("#@ args: {}", words.join(" ")));
    }
    if side(old, ".stdin").is_file() {
        let name = format!("{stem}.stdin");
        std::fs::rename(side(old, ".stdin"), new_dir.join(&name))
            .map_err(|e| Error::new(format!("{}: {e}", old.display())))?;
        directives.push(format!("#@ stdin: {name}"));
    }
    if text.contains("Ruby::Box.new") {
        directives.push("#@ env: RUBY_BOX=1".into());
    }
    if text.contains("Ruby::Box") {
        directives.push("#@ ruby: -W:no-experimental".into());
    }
    let mut zeo_env = Vec::new();
    if side(old, ".gc").is_file() {
        zeo_env.push("ZEO_GC=1");
        for line in
            String::from_utf8_lossy(&std::fs::read(side(old, ".gc")).unwrap_or_default()).lines()
        {
            if !line.trim().is_empty() {
                comments.push(format!("# {}", line.trim_start_matches('#').trim()));
            }
        }
    }
    if side(old, ".leakcheck").is_file() {
        zeo_env.push("ZEO_RT_LEAKCHECK=1");
    }
    if !zeo_env.is_empty() {
        directives.push(format!("#@ zeo-env: {}", zeo_env.join(" ")));
    }
    match old_dir_name.as_str() {
        "macos" => directives.push("#@ only: macos".into()),
        "jit" => directives.push("#@ backend: jit".into()),
        _ => {}
    }
    if let Some(gccheck) = read_opt(&side(old, ".gccheck"))? {
        for line in String::from_utf8_lossy(&gccheck).lines() {
            if line.starts_with("cycle leak: ") {
                directives.push(format!("#@ gccheck: {}", line.trim_end()));
            } else if !line.trim().is_empty() {
                comments.push(format!("# {}", line.trim_start_matches('#').trim()));
            }
        }
    }
    if side(old, ".pkggap").is_file() {
        directives.push("#@ pkggap".into());
    }

    // The program, with shared-fixture paths rewritten for its new depth.
    let mut program = text;
    if depth >= 2 {
        let up = "../".repeat(depth - 1);
        program = program
            .replace("\"../fixtures/", &format!("\"{up}fixtures/"))
            .replace("\"fixtures/", &format!("\"{up}fixtures/"));
    }
    let mut lines: Vec<&str> = program.split_inclusive('\n').collect();
    let magic = lines.iter().take_while(|l| is_magic(l.as_bytes())).count();
    let mut out = String::new();
    for l in lines.drain(..magic) {
        out.push_str(l);
    }
    if !directives.is_empty() || !comments.is_empty() {
        for c in &comments {
            out.push_str(c);
            out.push('\n');
        }
        for d in &directives {
            out.push_str(d);
            out.push('\n');
        }
    }
    for l in lines {
        out.push_str(l);
    }
    let program = out.into_bytes();

    // The trailer.
    let expected = read_opt(&side(old, ".expected"))?;
    let bytes = match expected {
        None => program,
        Some(stdout) => {
            let stderr = read_opt(&side(old, ".err.expected"))?.unwrap_or_default();
            let linux = match read_opt(&side(old, ".linux.expected"))? {
                Some(stdout) => Some(Answer {
                    stdout,
                    stderr: read_opt(&side(old, ".linux.err.expected"))?.unwrap_or_default(),
                    exit: Default::default(),
                }),
                None => None,
            };
            let trailer = Trailer {
                answer: Answer {
                    stdout,
                    stderr,
                    exit: Default::default(),
                },
                linux,
            };
            Case::render(&program, &trailer)
        }
    };
    Case::parse(&bytes).map_err(|e| Error::new(format!("{}: {e}", new.display())))?;
    std::fs::write(new, &bytes).map_err(|e| Error::new(format!("{}: {e}", new.display())))?;
    remove_program(old)?;

    // The program's own fixture directory.
    let fixture = old_dir.join(&stem);
    if fixture.is_dir() {
        let new_stem = new
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        move_dir(&fixture, &new_dir.join(new_stem))?;
    }
    Ok(())
}

fn leftovers(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut names: BTreeMap<PathBuf, bool> = BTreeMap::new();
    for e in entries {
        let e = e.map_err(|e| Error::new(e.to_string()))?;
        names.insert(e.path(), e.path().is_dir());
    }
    for (p, is_dir) in names {
        if is_dir {
            leftovers(&p, out)?;
        } else if p.file_name().is_some_and(|n| n != ".DS_Store") {
            out.push(p);
        }
    }
    Ok(())
}
