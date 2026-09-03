//! Stage and build the platform gem -- the second of the two artifacts one
//! payload feeds.
//!
//! `dist` already assembles everything a gem needs; this rearranges that one
//! staging into RubyGems' shape rather than building anything a second time:
//!
//! ```text
//!   dist                            gem
//!   bin/zeo                    ->   libexec/zeo
//!   share/zeo/**               ->   share/zeo/**        (verbatim)
//!   share/doc/zeo/README.md    ->   README.md
//!   --                         ->   exe/zeo             (the Ruby launcher)
//!   --                         ->   zeo.gemspec
//! ```
//!
//! `bin/` becomes `libexec/` because RubyGems binstubs an executable by
//! `load`ing it as Ruby, and zeo is a native binary. `exe/zeo` is the Ruby
//! that gets loaded, and it `exec`s the real one. Both sit two levels above
//! `share/zeo`, so `zeo::home`'s executable-relative probe finds the payload
//! in the gem exactly as it does in the tarball -- no gem-aware code in the
//! compiler, and no `ZEO_HOME`.

use std::path::{Path, PathBuf};

use crate::exec::{self, Capture};
use crate::{Error, root_join};

const USAGE: &str = "usage: cargo xtask gem [--dist <dir>] [--platform <name>] \
                     [--stage-only] [-o <dir>]";

/// Copied to the gem root, where `gem install` and rubygems.org both look.
const DOCS: &[&str] = &[
    "README.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "THIRD-PARTY-NOTICES.md",
];

struct Opts {
    dist: Option<PathBuf>,
    platform: Option<String>,
    stage_only: bool,
    out: Option<PathBuf>,
}

pub fn run(args: &[String]) -> Result<(), Error> {
    let Some(opts) = parse(args)? else {
        return Ok(());
    };
    let dist = match &opts.dist {
        Some(dir) => dir.clone(),
        None => sole_dist_staging()?,
    };
    let manifest = read_manifest(&dist)?;
    let platform = match &opts.platform {
        Some(p) => p.clone(),
        None => gem_platform(&manifest.target)?,
    };

    let out_dir = opts.out.clone().unwrap_or_else(|| root_join("target/gem"));
    let stage = out_dir.join(format!("zeo-{}-{platform}", manifest.version));
    refuse_to_delete_our_own_cwd(&stage)?;
    match std::fs::remove_dir_all(&stage) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::new(format!("clearing {}: {e}", stage.display()))),
    }

    copy_tree(&dist.join("share/zeo"), &stage.join("share/zeo"))?;
    copy(&dist.join("bin/zeo"), &stage.join("libexec/zeo"))?;
    copy(&root_join("exe/zeo"), &stage.join("exe/zeo"))?;
    copy(&root_join("zeo.gemspec"), &stage.join("zeo.gemspec"))?;
    for name in DOCS {
        let staged = dist.join("share/doc/zeo").join(name);
        let src = if staged.is_file() {
            staged
        } else {
            root_join(name)
        };
        if src.is_file() {
            copy(&src, &stage.join(name))?;
        }
    }
    make_executable(&stage.join("libexec/zeo"))?;
    make_executable(&stage.join("exe/zeo"))?;

    if opts.stage_only {
        println!("gem: staged {} (stage-only)", stage.display());
        return Ok(());
    }
    build(&stage, &platform)
}

/// Run `gem build` inside the staging directory.
///
/// The `gem` on PATH, not zeo's own: this is the tool that produces what zeo
/// ships, so it must not depend on zeo being correct. (`zeo gem build` on the
/// same directory is a worthwhile self-hosting proof, and a separate one.)
fn build(stage: &Path, platform: &str) -> Result<(), Error> {
    let out = exec::run(
        &["gem", "build", "zeo.gemspec"],
        stage,
        &[("ZEO_GEM_PLATFORM", Some(platform))],
        Capture::Both,
    )?;
    if !out.success() {
        return Err(Error::new(format!(
            "gem build failed:\n{}",
            out.stderr_text().trim()
        )));
    }
    let mut built: Vec<PathBuf> = std::fs::read_dir(stage)
        .map_err(|e| Error::new(format!("reading {}: {e}", stage.display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "gem"))
        .collect();
    built.sort();
    let Some(gem) = built.pop() else {
        return Err(Error::new(format!(
            "gem build wrote no .gem in {}",
            stage.display()
        )));
    };
    let bytes = std::fs::metadata(&gem).map(|m| m.len()).unwrap_or(0);
    println!(
        "gem: {} ({:.1} MB)",
        gem.display(),
        bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

struct Manifest {
    version: String,
    target: String,
}

fn read_manifest(dist: &Path) -> Result<Manifest, Error> {
    let path = dist.join("share/zeo/dist-manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::new(format!(
            "reading {}: {e}\n{} is not a `cargo xtask dist` staging directory",
            path.display(),
            dist.display()
        ))
    })?;
    let field = |name: &str| -> Result<String, Error> {
        text.split(&format!("\"{name}\""))
            .nth(1)
            .and_then(|rest| rest.split('"').nth(1))
            .map(str::to_string)
            .ok_or_else(|| Error::new(format!("{} states no {name}", path.display())))
    };
    Ok(Manifest {
        version: field("version")?,
        target: field("target")?,
    })
}

/// The one `zeo-<version>-<triple>/` under `target/dist`, so the common case
/// needs no `--dist`. Two of them is ambiguous rather than a guess.
fn sole_dist_staging() -> Result<PathBuf, Error> {
    let dir = root_join("target/dist");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| {
            Error::new(format!(
                "reading {}: {e}\nrun `cargo xtask dist` first, or pass --dist",
                dir.display()
            ))
        })?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("share/zeo/dist-manifest.json").is_file())
        .collect();
    found.sort();
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(Error::new(format!(
            "no dist staging under {} -- run `cargo xtask dist`",
            dir.display()
        ))),
        _ => Err(Error::new(format!(
            "{} holds {} dist stagings; name one with --dist",
            dir.display(),
            found.len()
        ))),
    }
}

/// A rust target triple as RubyGems spells the same machine.
///
/// RubyGems' platform vocabulary is `<cpu>-<os>[-<version>]` and is NOT
/// derivable from a triple by string surgery -- `aarch64-apple-darwin` is
/// `arm64-darwin`, and `x86_64-unknown-linux-gnu` is `x86_64-linux`. The
/// table is the four targets a release builds; anything else has to be named
/// with `--platform`, because guessing produces a gem RubyGems will never
/// select and nothing would say so.
fn gem_platform(triple: &str) -> Result<String, Error> {
    let row = match triple {
        "aarch64-apple-darwin" => "arm64-darwin",
        "x86_64-apple-darwin" => "x86_64-darwin",
        "x86_64-unknown-linux-gnu" => "x86_64-linux",
        "aarch64-unknown-linux-gnu" => "aarch64-linux",
        other => {
            return Err(Error::new(format!(
                "no RubyGems platform known for the target {other:?} -- pass \
                 --platform <name> (`ruby -e 'p Gem::Platform.local.to_s'` on \
                 that machine says what to use)"
            )));
        }
    };
    Ok(row.to_string())
}

fn parse(args: &[String]) -> Result<Option<Opts>, Error> {
    let mut opts = Opts {
        dist: None,
        platform: None,
        stage_only: false,
        out: None,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| Error::new(format!("{name} wants a value\n{USAGE}")))
        };
        match arg.as_str() {
            "--dist" => opts.dist = Some(PathBuf::from(value("--dist")?)),
            "--platform" => opts.platform = Some(value("--platform")?),
            "--stage-only" => opts.stage_only = true,
            "-o" => opts.out = Some(PathBuf::from(value("-o")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => return Err(Error::new(format!("unknown option {other:?}\n{USAGE}"))),
        }
    }
    Ok(Some(opts))
}

fn refuse_to_delete_our_own_cwd(stage: &Path) -> Result<(), Error> {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok());
    let target = stage.canonicalize().ok();
    if let (Some(cwd), Some(target)) = (cwd, target)
        && cwd.starts_with(&target)
    {
        return Err(Error::new(format!(
            "refusing to clear {}: the current directory is inside it",
            stage.display()
        )));
    }
    Ok(())
}

fn copy(src: &Path, dest: &Path) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::new(format!("creating {}: {e}", parent.display())))?;
    }
    std::fs::copy(src, dest).map_err(|e| {
        Error::new(format!(
            "copying {} to {}: {e}",
            src.display(),
            dest.display()
        ))
    })?;
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), Error> {
    let entries = std::fs::read_dir(src)
        .map_err(|e| Error::new(format!("reading {}: {e}", src.display())))?;
    std::fs::create_dir_all(dest)
        .map_err(|e| Error::new(format!("creating {}: {e}", dest.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", src.display())))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| Error::new(format!("chmod {}: {e}", path.display())))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every target the release builds has a RubyGems spelling.
    ///
    /// The two lists drift in one direction and it is silent until a tag is
    /// cut: a row added to the release matrix builds its tarball, and then
    /// `cargo xtask gem` refuses at the end of a four-way build because
    /// nothing here knows what to call it.
    #[test]
    fn every_release_target_has_a_gem_platform() {
        let text = std::fs::read_to_string(root_join(".github/workflows/release.yml"))
            .expect("the release workflow is committed");
        let targets: Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("target: "))
            .filter(|t| !t.contains("${{"))
            .collect();
        assert_eq!(
            targets.len(),
            4,
            "expected the four release targets, found {targets:?}"
        );
        for target in targets {
            gem_platform(target).unwrap_or_else(|e| panic!("{target}: {e}"));
        }
    }

    /// `exe/zeo` is the file RubyGems binstubs, and it is Ruby. If it ever
    /// became the binary itself, `gem install` would produce a `zeo` that
    /// dies inside `load`.
    #[test]
    fn the_gem_executable_is_ruby_that_execs_the_binary() {
        let launcher = std::fs::read_to_string(root_join("exe/zeo")).expect("exe/zeo is committed");
        assert!(launcher.starts_with("#!"), "exe/zeo has no shebang");
        assert!(
            launcher.contains("libexec/zeo"),
            "exe/zeo does not name the real binary"
        );
        assert!(
            launcher.contains("exec("),
            "exe/zeo does not exec -- a wrapper process would swallow signals"
        );
    }
}
