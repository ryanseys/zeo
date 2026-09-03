//! `zeo flags`: what a build of zeo turned on, for a person or a script.

use std::path::PathBuf;

use super::*;

/// `zeo flags [--json] [--bundle-gemfile <path>] [--gem-path <dir>]...`.
///
/// One producer with `zeo install` (`crate::gems::project`), so a Makefile that
/// captures `$(zeo flags)` cannot drift from what the verbs would do.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct FlagsCmd {
    pub(crate) gemfile: Option<PathBuf>,
    pub(crate) gem_paths: Vec<PathBuf>,
    pub(crate) json: bool,
}

/// The flags verb's own flags. Same shape as [`parse_install`].
pub(crate) fn parse_flags(argv: &[String]) -> Result<FlagsCmd, String> {
    let mut cmd = FlagsCmd::default();
    let mut iter = argv.iter();
    while let Some(arg) = iter.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let mut value = |flag: &str| -> Result<String, String> {
            match inline.clone() {
                Some(v) => Ok(v),
                None => iter
                    .next()
                    .cloned()
                    .ok_or(format!("{flag} requires a value")),
            }
        };
        match name {
            "--json" => cmd.json = true,
            "--bundle-gemfile" => cmd.gemfile = Some(PathBuf::from(value("--bundle-gemfile")?)),
            "--gem-path" => cmd.gem_paths.push(PathBuf::from(value("--gem-path")?)),
            _ => {
                return Err(format!(
                    "invalid option for zeo flags: {arg} (it takes --json, --bundle-gemfile \
                     and --gem-path)"
                ));
            }
        }
    }
    Ok(cmd)
}

pub(crate) fn run_flags(cmd: FlagsCmd) -> Result<(), MainError> {
    init_tracing(None);
    let env = Env::from_process();
    let gemfile = cmd
        .gemfile
        .or_else(|| env.bundle_gemfile.as_ref().map(PathBuf::from));
    let project = crate::gems::project::locate(
        gemfile,
        cmd.gem_paths,
        std::env::var_os("GEM_PATH").as_deref(),
    )?;
    let rows = crate::gems::project::survey(&project)?;
    let abs = |p: &std::path::Path| {
        p.canonicalize()
            .unwrap_or_else(|_| p.to_path_buf())
            .display()
            .to_string()
    };
    if cmd.json {
        let json = serde_json::json!({
            "gemfile": abs(&project.gemfile),
            "lockfile": abs(&project.lockfile),
            "stores": project.stores.iter().map(|s| abs(s)).collect::<Vec<_>>(),
            "gems": rows.iter().map(|r| serde_json::json!({
                "name": r.name,
                "version": r.version,
                "feature": r.feature,
                "artifact": r.home.as_ref().filter(|h| h.is_file()).map(|h| abs(h)),
                "skip": r.skip,
            })).collect::<Vec<_>>(),
        });
        println!("{json:#}");
        return Ok(());
    }
    let mut words = Vec::new();
    for store in &project.stores {
        words.push("--gem-path".to_string());
        words.push(shell_quote(&abs(store)));
    }
    words.push("--bundle-gemfile".to_string());
    words.push(shell_quote(&abs(&project.gemfile)));
    for artifact in crate::gems::project::linkable(&rows) {
        words.push("--with-package".to_string());
        words.push(shell_quote(&abs(&artifact)));
    }
    println!("{}", words.join(" "));
    Ok(())
}

/// Single-quote `s` for a POSIX shell; an embedded quote becomes `'\''`.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
