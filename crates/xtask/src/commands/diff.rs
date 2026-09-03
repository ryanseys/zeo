//! Run a ruby snippet through BOTH engines, show each side, and say whether
//! they agree. On a DIVERGENCE it files the snippet under `todo/` with
//! ruby's answer recorded under `__END__`; on a MATCH it writes nothing.
//!
//! This is the long-tail loop in one command: find a divergence, file it
//! with the answer zeo has to reach.
//!
//! `todo/` is not run by anything. When the program starts matching, it
//! moves into the topic directory it belongs to under `test/` and becomes an
//! ordinary test.

use std::io::Read;

use crate::exec::{self, Capture};
use crate::ruby::Oracle;
use crate::{Error, root, root_join};

const USAGE: &str = "\
usage: cargo xtask diff [<code> | -f <file> | -]

  cargo xtask diff 'puts 1 + 1'          # inline code
  cargo xtask diff -f snippet.rb         # from a file
  pbpaste | cargo xtask diff -           # from stdin
  --name NAME / -n NAME                  # gap stem to use on divergence
  --no-gap                               # compare only; never write a gap
";

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut name = "gap_snippet".to_string();
    let mut gap = true;
    let mut file = None;
    let mut inline = None;
    let mut from_stdin = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| Error::new(format!("{flag} wants a value\n\n{USAGE}")))
        };
        match arg.as_str() {
            "--name" | "-n" => name = value("--name")?,
            "-f" => file = Some(value("-f")?),
            "--no-gap" => gap = false,
            "-" => from_stdin = true,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            other if other.starts_with("--") => {
                return Err(Error::new(format!("unknown option {other:?}\n\n{USAGE}")));
            }
            other => inline = Some(other.to_string()),
        }
    }

    let source = read_source(file.as_deref(), inline.as_deref(), from_stdin)?
        .ok_or_else(|| Error::new(USAGE.to_string()))?;
    println!("=== snippet ===");
    print!("{source}");
    println!();

    let oracle = Oracle::find();
    let ruby = exec::run_with_stdin(
        &oracle.argv(&["-"]),
        root(),
        &oracle.env(),
        Capture::Both,
        Some(source.as_bytes()),
    )?;
    let zeo_bin = crate::build_zeo()?;
    let zeo = exec::run(
        &[
            zeo_bin.as_os_str(),
            std::ffi::OsStr::new("-W0"),
            std::ffi::OsStr::new("-e"),
            std::ffi::OsStr::new(&source),
        ],
        root(),
        &[],
        Capture::Both,
    )?;
    show("ruby", &ruby);
    println!();
    show("zeo", &zeo);
    println!();

    // A quick verdict on stdout plus exit; the harness reconciles the fine
    // print.
    if ruby.stdout == zeo.stdout && ruby.code == zeo.code {
        println!(
            "MATCH (stdout + exit). Not a gap -- belongs in a test/ topic dir if you want to keep it."
        );
        return Ok(());
    }
    println!("DIVERGE:");
    if ruby.code != zeo.code {
        println!(
            "  ruby exit {}, zeo exit {}",
            ruby.code_text(),
            zeo.code_text()
        );
    }
    show_stdout_diff(&ruby.stdout_text(), &zeo.stdout_text());
    println!();
    if !gap {
        println!("(--no-gap: not writing a gap file)");
        return Ok(());
    }
    write_gap(&name, &source)
}

fn read_source(
    file: Option<&str>,
    inline: Option<&str>,
    from_stdin: bool,
) -> Result<Option<String>, Error> {
    if let Some(path) = file {
        return std::fs::read_to_string(path)
            .map(Some)
            .map_err(|e| Error::new(format!("reading {path}: {e}")));
    }
    if from_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| Error::new(format!("reading stdin: {e}")))?;
        return Ok(Some(buf));
    }
    Ok(inline.map(|s| format!("{s}\n")))
}

fn show(label: &str, out: &exec::Output) {
    println!("--- {label} (exit {}) ---", out.code_text());
    println!("stdout:");
    print!("{}", out.stdout_text());
    if out.stderr.is_empty() {
        return;
    }
    println!("stderr:");
    print!("{}", out.stderr_text());
}

fn show_stdout_diff(a: &str, b: &str) {
    let al: Vec<&str> = a.lines().collect();
    let bl: Vec<&str> = b.lines().collect();
    for i in 0..al.len().max(bl.len()) {
        if al.get(i) == bl.get(i) {
            continue;
        }
        println!("  ruby {}: {:?}", i + 1, al.get(i));
        println!("  zeo  {}: {:?}", i + 1, bl.get(i));
    }
}

fn write_gap(name: &str, source: &str) -> Result<(), Error> {
    let name = name.trim_end_matches(".rb");
    let dest = root_join("todo").join(format!("{name}.rb"));
    if dest.exists() {
        return Err(Error::new(format!(
            "refusing to overwrite todo/{name}.rb (use --name)"
        )));
    }
    std::fs::write(&dest, source)
        .map_err(|e| Error::new(format!("writing {}: {e}", dest.display())))?;
    // Through `bless` -- the ONE writer of a recorded answer -- so the output
    // gets the same normalization a test's does (CRLF, source-path
    // relativization, address scrubbing). A hand-rolled oracle capture here
    // once skipped the address scrub, so a snippet printing `#<Object:0x...>`
    // recorded a raw process-random address.
    super::bless::bless(&[&format!("todo::{name}.rb")], false)?;
    println!("wrote todo/{name}.rb, with ruby's answer under __END__");
    println!();
    println!("Nothing runs it. When zeo matches, move it into a topic dir:");
    println!("  git mv todo/{name}.rb test/<topic>/<area>/{name}.rb");
    Ok(())
}
