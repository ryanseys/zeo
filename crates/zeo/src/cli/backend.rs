//! `zeo backend`: link CLIF text another front end wrote.

use std::path::PathBuf;

use super::*;

/// `zeo backend <file.clif> [--data <file.zeodata>] -o <binary>`.
#[derive(Debug, PartialEq)]
pub(crate) struct BackendCmd {
    pub(crate) clif: PathBuf,
    pub(crate) data: Option<PathBuf>,
    pub(crate) output: PathBuf,
}

/// The install verb's own flags: a store, a Gemfile, gem names. `-h` gets
/// the main help; anything else is refused by name.
pub(crate) fn parse_backend(argv: &[String]) -> Result<BackendCmd, String> {
    let (mut clif, mut data, mut output) = (None, None, None);
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
            "--data" => data = Some(PathBuf::from(value("--data")?)),
            "-o" => output = Some(PathBuf::from(value("-o")?)),
            _ if arg.starts_with('-') => {
                return Err(format!(
                    "invalid option for zeo backend: {arg} (it takes --data and -o)"
                ));
            }
            _ if clif.is_none() => clif = Some(PathBuf::from(arg)),
            _ => {
                return Err(format!(
                    "zeo backend takes one CLIF file; `{arg}` is a second"
                ));
            }
        }
    }
    Ok(BackendCmd {
        clif: clif.ok_or("zeo backend needs a CLIF file")?,
        data,
        output: output.ok_or("zeo backend needs -o <binary>")?,
    })
}

/// `zeo flags`: print exactly the flags a compile of this project implies.
///
/// Default: one shell-quoted line for `$(zeo flags)` in a Makefile.
/// `--json` prints the structured form instead -- the lockfile, the
/// stores, and one row per gem with its artifact path or the reason it
/// has none. The producer is the same `crate::gems::project` survey the compile
/// itself consults, so the handoff cannot drift.
pub(crate) fn run_backend(cmd: BackendCmd) -> Result<(), MainError> {
    init_tracing(None);
    crate::backend::clif_text::build(&cmd.clif, cmd.data.as_deref(), &cmd.output)?;
    Ok(())
}
