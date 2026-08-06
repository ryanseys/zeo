//! `cargo xtask sweep` -- what is taking the disk, and what to do about it.
//!
//! `target/` reached 91GB once before anyone looked, and there is no config
//! that prevents it: the variant target dirs, the golden suite's test-binary
//! cache and the probe's unpacked gem cache are all doing their jobs. What was
//! missing is a way to SEE the split without assembling five `du` invocations
//! by hand, which is why the answer to "is it time yet" was always a guess.
//!
//! This never deletes anything, and takes no flag that would. Two reasons, and
//! the second is the real one: a wipe costs a full rebuild, so it wants a human
//! deciding when -- and a tool that can delete gets run by reflex, which is how
//! you lose a cache you were about to need. It prints the commands instead.

use std::path::Path;
use std::process::{Command, ExitCode};

/// Kilobytes on disk, via `du`. Walking the tree in Rust would count the same
/// bytes far more slowly -- `target/` alone is hundreds of thousands of files.
fn size_kb(path: &Path) -> Option<u64> {
    if !path.exists() {
        return None;
    }
    let out = Command::new("du")
        .arg("-sk")
        .arg(path)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn human(kb: u64) -> String {
    const UNITS: [(&str, u64); 3] = [("GB", 1024 * 1024), ("MB", 1024), ("KB", 1)];
    for (unit, scale) in UNITS {
        if kb >= scale {
            return format!("{:.1} {unit}", kb as f64 / scale as f64);
        }
    }
    "0 KB".to_string()
}

/// Direct children of `dir`, largest first -- the shape that answers "which
/// variant is it" without a second command.
fn children(dir: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, u64)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let kb = size_kb(&e.path())?;
            Some((e.file_name().to_string_lossy().into_owned(), kb))
        })
        .collect();
    out.sort_by_key(|&(_, kb)| std::cmp::Reverse(kb));
    out
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    if let Some(bad) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!(
            "sweep: unknown option {bad:?} -- this command only reports.\n\
             It prints the `rm` commands rather than running them; a wipe costs\n\
             a full rebuild, so it wants a human deciding when."
        );
        return ExitCode::FAILURE;
    }

    let mut total = 0u64;
    println!("{:<28} {:>10}", "PATH", "SIZE");
    for rel in ["target", "vendor/gems", "vendor/.probe"] {
        let Some(kb) = size_kb(&root.join(rel)) else {
            continue;
        };
        total += kb;
        println!("{rel:<28} {:>10}", human(kb));
    }
    println!("{:<28} {:>10}", "", "-------");
    println!("{:<28} {:>10}", "total", human(total));

    let target = root.join("target");
    let variants = children(&target);
    if !variants.is_empty() {
        println!("\ntarget/ by variant:");
        for (name, kb) in variants.iter().take(8) {
            println!("  {name:<26} {:>10}", human(*kb));
        }
    }

    println!(
        "\nNothing here is waste -- the variant dirs, the golden suite's test-binary\n\
         cache and the probe's unpacked gems are each earning their space. The\n\
         question is only whether you want the rebuild back:\n\
         \n  \
         rm -rf {}/target          # full rebuild, frees the most\n  \
         rm -rf {}/vendor/.probe   # rebuilt per gem on the next probe, cheap\n  \
         rm -rf {}/vendor/gems     # re-downloads every probed gem, slow",
        root.display(),
        root.display(),
        root.display(),
    );
    ExitCode::SUCCESS
}
