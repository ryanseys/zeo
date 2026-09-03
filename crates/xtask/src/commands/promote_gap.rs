//! Promote a FIXED gap out of `test/gaps/` into the topic directory it
//! belongs to.
//!
//! When a gap starts matching ruby, the harness fails it with "GAP FIXED --
//! promote". A gap has no topic of its own, so the caller names one:
//! `cargo xtask promote-gap <stem> core/string`. The program, its recorded
//! answer and its fixture directory move together, and the promoted case is
//! run once to confirm it passes.

use crate::exec::{self, Capture};
use crate::suites::Suite;
use crate::{Error, root, root_join};

const USAGE: &str = "usage: cargo xtask promote-gap <gap-stem> <topic/area>   (e.g. core/string)";

pub fn run(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let (Some(stem), Some(area)) = (args.first(), args.get(1)) else {
        return Err(Error::new(USAGE.to_string()));
    };
    let stem = stem.trim_end_matches(".rb");
    let (topic, sub) = area
        .trim_matches('/')
        .split_once('/')
        .ok_or_else(|| Error::new(format!("{area:?} is not <topic>/<area>\n{USAGE}")))?;
    let suite = Suite::by_name(topic)
        .filter(|s| s.depth == crate::suites::Depth::Two)
        .ok_or_else(|| {
            Error::new(format!(
                "{topic:?} is not a topic (lang, core, stdlib, compiler)"
            ))
        })?;
    let gaps = root_join("test/gaps");
    let from = gaps.join(format!("{stem}.rb"));
    if !from.is_file() {
        return Err(Error::new(format!("no such gap: test/gaps/{stem}.rb")));
    }
    let dest_dir = root_join(suite.root).join(sub);
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| Error::new(format!("{}: {e}", dest_dir.display())))?;
    let to = dest_dir.join(format!("{stem}.rb"));
    if to.exists() {
        return Err(Error::new(format!("{} already exists", to.display())));
    }
    std::fs::rename(&from, &to).map_err(|e| Error::new(format!("{}: {e}", to.display())))?;
    let fixture = gaps.join(stem);
    if fixture.is_dir() {
        std::fs::rename(&fixture, dest_dir.join(stem))
            .map_err(|e| Error::new(format!("moving {}: {e}", fixture.display())))?;
    }
    let rel = format!("{sub}/{stem}.rb");
    println!("promoted test/gaps/{stem}.rb -> {}/{rel}", suite.root);

    println!("verifying it passes as {}::{rel} ...", suite.name);
    let out = exec::run(
        &[
            "cargo",
            "nextest",
            "run",
            "-p",
            "zeo",
            "--test",
            "corpus",
            "-E",
            &format!("test({}::{rel})", suite.name),
        ],
        root(),
        &[],
        Capture::Nothing,
    )?;
    if !out.success() {
        // Put it back. A half-promoted tree is worse than a refusal: the
        // program would sit in a topic directory failing as an ordinary test.
        std::fs::rename(&to, &from).map_err(|e| Error::new(format!("{}: {e}", from.display())))?;
        if dest_dir.join(stem).is_dir() {
            std::fs::rename(dest_dir.join(stem), &fixture)
                .map_err(|e| Error::new(format!("moving {}: {e}", fixture.display())))?;
        }
        return Err(Error::new(format!(
            "{stem} does not pass in {} -- it is still a gap, and stays in test/gaps/",
            suite.root
        )));
    }
    Ok(())
}
