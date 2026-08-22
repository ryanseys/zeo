//! `--embed-sources`: the ruby source that travels INSIDE a program.
//!
//! The run-time loader resolves a `require` against this pack before it
//! looks at disk, which is what lets a hermetic binary answer a require its
//! compiler could not resolve -- a computed feature name, or a gem that
//! lives inside zeo's own tree and is on no machine the binary will run on.
//!
//! Empty by default, and deliberately: embedding every source a program can
//! see would double the artifact for a tier most programs never reach.

use std::path::{Path, PathBuf};

use crate::CompileError;

/// Every `.rb` under `roots`, keyed by its root-relative spelling with the
/// `.rb` stripped -- the name a `require` writes. First root wins, exactly
/// as `$LOAD_PATH` resolves.
pub(crate) fn collect(roots: &[PathBuf]) -> Result<Vec<(String, String)>, CompileError> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let root = root.canonicalize().map_err(|e| CompileError::Report {
            message: format!("--embed-sources {}: {e}", root.display()),
        })?;
        let mut files = Vec::new();
        walk(&root, &mut files)?;
        files.sort();
        for path in files {
            let Ok(rel) = path.strip_prefix(&root) else {
                continue;
            };
            let feature = rel
                .with_extension("")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if !seen.insert(feature.clone()) {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| CompileError::Report {
                message: format!("--embed-sources reading {}: {e}", path.display()),
            })?;
            out.push((feature, text));
        }
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CompileError> {
    let entries = std::fs::read_dir(dir).map_err(|e| CompileError::Report {
        message: format!("--embed-sources {}: {e}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| CompileError::Report {
            message: format!("--embed-sources {}: {e}", dir.display()),
        })?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rb") {
            out.push(path);
        }
    }
    Ok(())
}
