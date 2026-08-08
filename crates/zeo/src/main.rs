//! The `zeo` CLI: argument parsing plus calling into the `zeo`
//! library's `compile_to_rust`/`backend::build_binary` -- see `lib.rs` for the
//! actual parse -> analyze -> codegen -> build pipeline.

use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

/// What `run` can fail with: a compile error renders as a miette diagnostic
/// (annotated source excerpt, auto-degrading for pipes/NO_COLOR); everything
/// else (argument parsing, IO, the `cargo`/`rustc` build step) keeps the
/// plain `zeo: <msg>` line.
enum MainError {
    Plain(String),
    Compile(zeo::CompileError),
}

impl From<String> for MainError {
    fn from(msg: String) -> MainError {
        MainError::Plain(msg)
    }
}

impl From<zeo::CompileError> for MainError {
    fn from(err: zeo::CompileError) -> MainError {
        MainError::Compile(err)
    }
}

struct Args {
    /// The input source: either a `.rb` file path or, with `-e`, an inline
    /// program string (`ruby -e`'s shape). Exactly one is required.
    source: Source,
    output: Option<PathBuf>,
    /// `--dump=rust`: print the generated Rust source instead of building.
    dump_rust: bool,
    /// `-I` roots, then RUBYOPT's `-I` roots, then RUBYLIB -- ruby's order.
    load_roots: Vec<PathBuf>,
    /// `--gems <dir>`: vendored-gem directories (repeatable).
    package_dirs: Vec<PathBuf>,
    /// `--report[=<path>]`: the `zeo-gems.json` disclosure record, opt-in.
    report: Report,
    /// Warning categories suppressed via `-W0`/`-W:no-<category>`.
    nowarn: HashSet<String>,
    /// The external gem store dirs (`--gem-path`/`GEM_PATH`) and the lockfile
    /// derived from `--bundle-gemfile`/`BUNDLE_GEMFILE`. Either both are
    /// populated or neither -- `parse_args_from` enforces the pairing.
    gem_paths: Vec<PathBuf>,
    lockfile: Option<PathBuf>,
    /// Whether a store FLAG was given (vs env-only): flags demand a strict
    /// "lockfile must exist" check, ambient env degrades quietly.
    store_from_flags: bool,
    /// `--log-level <off|error|warn|info|debug|trace>`: install a `tracing`
    /// subscriber for the compiler at this level (overrides `ZEO_LOG`/`RUST_LOG`).
    log_level: Option<String>,
    /// `--run`: compile the input FILE and execute it straight away, the way
    /// `-e` already does for inline code.
    ///
    /// Running is strictly OPT-IN and stays that way. zeo compiles code that
    /// has not necessarily been read yet, so naming a file must never be
    /// enough to execute it -- nothing implies this flag, and a bare
    /// `zeo foo.rb` still only compiles.
    run: bool,
    /// ARGV for an immediately-run program (`-e`, or a file with `--run`):
    /// positionals and everything after `--`, exactly ruby's
    /// `[--] [args...]` shape.
    program_args: Vec<String>,
}

enum Source {
    File(PathBuf),
    /// `-e <code>` (repeatable; joined with newlines, like `ruby -e`). Compiles
    /// AND runs immediately, forwarding stdout/stderr and the exit status --
    /// the shape the `ruby`-differential harness drives.
    Eval(String),
}

/// The `zeo-gems.json` disclosure record: off unless `--report` asked for it.
enum Report {
    Off,
    /// Bare `--report`: next to the output artifact.
    DefaultPath,
    /// `--report=<path>`.
    Path(PathBuf),
}

/// `parse_args_from`'s outcome: a compile to run, or an informational mode
/// the wrapper prints and exits for.
enum Parsed {
    Run(Box<Args>),
    /// `-h`/`--help` (exit 0).
    Help,
    /// `-v`/`--version` (exit 0).
    Version,
    /// Bare `zeo`: help, but exit 1 -- an invocation that compiled nothing
    /// must not look like success to a caller that expected an artifact.
    NoInput,
}

/// The environment `parse_args_from` consults -- captured as a value so the
/// parser is a pure function the unit tests can drive.
#[derive(Default)]
struct Env {
    rubyopt: Option<String>,
    rubylib: Option<std::ffi::OsString>,
    gem_path: Option<std::ffi::OsString>,
    bundle_gemfile: Option<std::ffi::OsString>,
}

impl Env {
    fn from_process() -> Env {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        Env {
            rubyopt: std::env::var("RUBYOPT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            rubylib: var("RUBYLIB"),
            gem_path: var("GEM_PATH"),
            bundle_gemfile: var("BUNDLE_GEMFILE"),
        }
    }
}

const HELP: &str = "\
zeo -- compile Ruby to a native binary

usage: zeo [options] [--] (<input.rb> | -e <code>) [args...]

modes:
  <input.rb>            compile the file to a native binary (default output:
                        the input path with its extension stripped)
  <input.rb> --run      compile the file and run it immediately, forwarding
                        stdout/stderr and the exit status; trailing [args...]
                        become the program's ARGV. Running is opt-in and
                        nothing implies it -- a bare <input.rb> only compiles
  -e <code>             compile and run an inline program immediately,
                        forwarding stdout/stderr and the exit status
                        (repeatable; snippets are joined with newlines);
                        trailing [args...] become the program's ARGV;
                        with -o, write the binary instead of running it

options:
  -o <output>           where to write the compiled binary
  -I <dir>              add a `require` search root, like ruby's -I
                        (repeatable; `-I<dir>` also accepted)
  --gems <dir>          add a directory of vendored gems: every subdirectory
                        with a `.gemspec` is discovered as a gem (repeatable)
  --gem-path <dir>      an installed RubyGems store (`gem env gemdir`) to
                        resolve locked gems against (repeatable; defaults to
                        GEM_PATH); needs a Gemfile via --bundle-gemfile
  --bundle-gemfile <path>
                        the Gemfile whose lockfile (`<path>.lock`) selects
                        the store gem versions (defaults to BUNDLE_GEMFILE);
                        needs a store via --gem-path
  --report[=<path>]     write the `zeo-gems.json` disclosure record
                        (default path: next to the output artifact)
  -W0                   suppress all zeo warnings
  -W:no-<category>      suppress one warning category; `-W:<category>`
                        re-enables it. Categories: zeo-builtin-substitute
  --dump=rust           print the generated Rust source and exit (no build)
  --log-level <level>   log the compiler's internals to stderr at this level:
                        off|error|warn|info|debug|trace (overrides ZEO_LOG)
  -v, --version         print the version and exit
  -h, --help            show this message

Long options also accept the attached `--flag=<value>` spelling.

require search order (first gem with a given name wins):
  1. -I roots in the order given, then RUBYLIB entries
  2. --gems dirs in the order given
  3. the input file's sibling gems/ directory
  4. zeo's own bundled gems, then the external gem store

environment:
  RUBYOPT               extra leading options (only -I, -w and -W allowed)
  RUBYLIB               extra `require` search roots, after every -I
  GEM_PATH              gem store dirs for --gem-path; activates only when a
                        Gemfile is also known (an ambient store alone never
                        changes a compile)
  BUNDLE_GEMFILE        the Gemfile for --bundle-gemfile
  ZEO_RUNTIME_PROFILE   `debug` or `release` -- override the runtime profile
                        (default: debug for -e, release for -o compiles)
  ZEO_LOG / RUST_LOG    a `tracing` EnvFilter directive for finer control than
                        --log-level, e.g. `zeo::analyze=debug,zeo::lower=trace`
";

fn parse_args() -> Result<Parsed, String> {
    parse_args_from(std::env::args().skip(1).collect(), &Env::from_process())
}

fn parse_args_from(argv: Vec<String>, env: &Env) -> Result<Parsed, String> {
    let mut input = None;
    let mut eval: Option<String> = None;
    let mut output = None;
    let mut dump_rust = false;
    let mut load_roots = Vec::new();
    let mut package_dirs = Vec::new();
    let mut report = Report::Off;
    let mut nowarn = HashSet::new();
    let mut gem_paths: Vec<PathBuf> = Vec::new();
    let mut gemfile: Option<PathBuf> = None;
    let mut log_level = None;
    let mut run = false;
    let mut program_args: Vec<String> = Vec::new();

    // RUBYOPT first, so the command line wins wherever both touch the same
    // dial (ruby's precedence: `RUBYOPT=-W0 ruby -W2` is verbose). Only the
    // option subset that can't smuggle in a program is allowed, like ruby.
    let mut rubyopt_roots = Vec::new();
    if let Some(opt) = &env.rubyopt {
        let mut toks = opt.split_whitespace();
        while let Some(tok) = toks.next() {
            if tok == "-I" {
                let dir = toks.next().ok_or("-I in RUBYOPT requires a directory")?;
                rubyopt_roots.push(PathBuf::from(dir));
            } else if let Some(dir) = tok.strip_prefix("-I").filter(|d| !d.is_empty()) {
                rubyopt_roots.push(PathBuf::from(dir));
            } else if tok == "-w" || tok.starts_with("-W") {
                warn_flag(tok, &mut nowarn)?;
            } else {
                return Err(format!("illegal switch in RUBYOPT: {tok}"));
            }
        }
    }

    let mut iter = argv.into_iter();
    let mut after_dashdash = false;
    let mut collecting_argv = false;
    while let Some(arg) = iter.next() {
        // Once `-e` mode sees its first positional, everything that follows
        // is the program's ARGV -- ruby stops option parsing there too.
        if collecting_argv {
            program_args.push(arg);
            continue;
        }
        if after_dashdash {
            if eval.is_some() {
                program_args.push(arg);
            } else if input.is_none() {
                input = Some(PathBuf::from(arg));
            } else {
                // ARGV for a `--run` file; rejected below if nothing runs.
                program_args.push(arg);
            }
            continue;
        }
        if arg == "--" {
            after_dashdash = true;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--").filter(|r| !r.is_empty()) {
            let (name, inline) = match rest.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (rest, None),
            };
            // A required value: attached `--flag=<v>` or the next token.
            let mut value = |flag: &str| -> Result<String, String> {
                match inline.clone() {
                    Some(v) => Ok(v),
                    None => iter.next().ok_or(format!("{flag} requires a value")),
                }
            };
            match name {
                "help" => return Ok(Parsed::Help),
                "version" => return Ok(Parsed::Version),
                "run" => run = true,
                "gems" => package_dirs.push(PathBuf::from(value("--gems")?)),
                "gem-path" => gem_paths.push(PathBuf::from(value("--gem-path")?)),
                "bundle-gemfile" => {
                    gemfile = Some(PathBuf::from(value("--bundle-gemfile")?));
                }
                "log-level" => log_level = Some(validate_log_level(&value("--log-level")?)?),
                // Only the attached form takes a path -- a spaced value would
                // be ambiguous with the input file.
                "report" => {
                    report = match inline {
                        Some(path) => Report::Path(PathBuf::from(path)),
                        None => Report::DefaultPath,
                    };
                }
                "dump" => {
                    for item in value("--dump")?.split(',') {
                        match item {
                            "rust" => dump_rust = true,
                            other => {
                                return Err(format!(
                                    "--dump: unknown item `{other}` (expected rust)"
                                ));
                            }
                        }
                    }
                }
                // The pre-CRuby-convention spellings, kept as pointed errors
                // so an old script fails with the fix in hand.
                "packages" => return Err("--packages was renamed; use --gems <dir>".into()),
                "nowarn" => {
                    return Err("--nowarn was replaced; use -W:no-<category> \
                         (e.g. -W:no-zeo-builtin-substitute), or -W0 for all"
                        .into());
                }
                "lockfile" => {
                    return Err("--lockfile was replaced; use --bundle-gemfile <Gemfile> \
                         (its `<path>.lock` sibling is read)"
                        .into());
                }
                "no-report" => {
                    return Err(
                        "--no-report was removed; the report is opt-in now (see --report)".into(),
                    );
                }
                _ => {
                    return Err(format!(
                        "invalid option: {arg} (-h will show valid options)"
                    ));
                }
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            match arg.as_str() {
                "-o" => {
                    output = Some(PathBuf::from(iter.next().ok_or("-o requires a path")?));
                }
                // Inline program, `ruby -e` style: repeatable, lines joined.
                "-e" => {
                    let code = iter.next().ok_or("-e requires a code string")?;
                    match &mut eval {
                        Some(acc) => {
                            acc.push('\n');
                            acc.push_str(&code);
                        }
                        None => eval = Some(code),
                    }
                }
                // A `require` search root, like ruby's own -I (repeatable,
                // first hit wins in the order given).
                "-I" => {
                    load_roots.push(PathBuf::from(iter.next().ok_or("-I requires a directory")?));
                }
                "-h" => return Ok(Parsed::Help),
                "-v" => return Ok(Parsed::Version),
                "-S" => return Err("-S was replaced; use --dump=rust".into()),
                _ => {
                    // Attached `-I<dir>` (ruby's own spelling, no space).
                    if let Some(dir) = arg.strip_prefix("-I").filter(|d| !d.is_empty()) {
                        load_roots.push(PathBuf::from(dir));
                    } else if arg == "-w" || arg.starts_with("-W") {
                        warn_flag(&arg, &mut nowarn)?;
                    } else {
                        return Err(format!(
                            "invalid option: {arg} (-h will show valid options)"
                        ));
                    }
                }
            }
        } else if eval.is_some() {
            program_args.push(arg);
            collecting_argv = true;
        } else if input.is_none() {
            input = Some(PathBuf::from(arg));
        } else {
            // Everything after the script name is the program's ARGV, ruby's
            // shape -- kept for `--run` and rejected below if nothing runs.
            program_args.push(arg);
            collecting_argv = true;
        }
    }

    let source = match (eval, input) {
        (Some(_), Some(_)) => return Err("cannot combine -e with a file argument".to_string()),
        (Some(code), None) => Source::Eval(code),
        (None, Some(path)) => Source::File(path),
        (None, None) => return Ok(Parsed::NoInput),
    };
    // `--run` runs instead of producing an artifact, so `-o` contradicts it.
    if run && output.is_some() {
        return Err("--run executes the program instead of writing a binary; drop -o".to_string());
    }
    // Trailing args are ARGV, which only an immediately-run program has.
    let runs_now = (matches!(source, Source::Eval(_)) || run) && output.is_none();
    if !program_args.is_empty() && !runs_now {
        return Err(format!("unexpected argument `{}`", program_args[0]));
    }
    // Bare `--report` derives its path from the output artifact, which the
    // run-immediately `-e` mode does not have.
    if matches!(report, Report::DefaultPath)
        && matches!(source, Source::Eval(_))
        && output.is_none()
    {
        return Err("--report with -e has no artifact directory; use --report=<path>".to_string());
    }

    // The external gem store: flags first, env fills what flags left unset.
    // It activates only when BOTH a store and a Gemfile are known -- a flag
    // missing its counterpart is an error, but ambient env alone (GEM_PATH is
    // exported in every RubyGems shell) never changes a compile.
    let gem_paths_from_flag = !gem_paths.is_empty();
    let gemfile_from_flag = gemfile.is_some();
    if gem_paths.is_empty()
        && let Some(gp) = &env.gem_path
    {
        gem_paths = std::env::split_paths(gp).collect();
    }
    if gemfile.is_none()
        && let Some(bg) = &env.bundle_gemfile
    {
        gemfile = Some(PathBuf::from(bg));
    }
    if gem_paths_from_flag && gemfile.is_none() {
        return Err(
            "--gem-path needs a Gemfile: give --bundle-gemfile <path> or set BUNDLE_GEMFILE"
                .to_string(),
        );
    }
    if gemfile_from_flag && gem_paths.is_empty() {
        return Err(
            "--bundle-gemfile needs a gem store: give --gem-path <dir> or set GEM_PATH".to_string(),
        );
    }
    if gem_paths.is_empty() || gemfile.is_none() {
        gem_paths = Vec::new();
        gemfile = None;
    }

    Ok(Parsed::Run(Box::new(Args {
        source,
        output,
        run,
        dump_rust,
        load_roots: {
            let mut roots = load_roots;
            roots.extend(rubyopt_roots);
            if let Some(lib) = &env.rubylib {
                roots.extend(std::env::split_paths(lib));
            }
            roots
        },
        package_dirs,
        report,
        nowarn,
        store_from_flags: gem_paths_from_flag || gemfile_from_flag,
        gem_paths,
        lockfile: gemfile.map(derive_lockfile),
        log_level,
        program_args,
    })))
}

/// The `-w`/`-W` family, ruby's shapes: levels are accepted (`-W0` silences
/// everything; the rest are the default, already-on behavior), and
/// `-W:[no-]<category>` toggles one category, validated so a typo is an error
/// rather than a silently ignored suppression.
fn warn_flag(arg: &str, nowarn: &mut HashSet<String>) -> Result<(), String> {
    match arg {
        "-w" | "-W" | "-W1" | "-W2" => Ok(()),
        "-W0" => {
            for category in zeo::gem_report::WARNING_CATEGORIES {
                nowarn.insert((*category).to_string());
            }
            Ok(())
        }
        _ => match arg.strip_prefix("-W:") {
            Some(category) => {
                let (suppress, name) = match category.strip_prefix("no-") {
                    Some(name) => (true, name),
                    None => (false, category),
                };
                if !zeo::gem_report::WARNING_CATEGORIES.contains(&name) {
                    return Err(format!("unknown warning category: `{name}'"));
                }
                if suppress {
                    nowarn.insert(name.to_string());
                } else {
                    nowarn.remove(name);
                }
                Ok(())
            }
            None => Err(format!(
                "invalid option: {arg} (-h will show valid options)"
            )),
        },
    }
}

/// The lockfile a Gemfile path names, by bundler's own rules: `Gemfile` ->
/// `Gemfile.lock`, `gems.rb` -> `gems.locked`, anything else gets `.lock`
/// appended; a path that already IS a lockfile is taken as given.
fn derive_lockfile(gemfile: PathBuf) -> PathBuf {
    let name = gemfile
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.ends_with(".lock") || name.ends_with(".locked") {
        gemfile
    } else if name == "gems.rb" {
        gemfile.with_file_name("gems.locked")
    } else {
        gemfile.with_file_name(format!("{name}.lock"))
    }
}

/// Accept only the standard `tracing` levels, so a typo (`--log-level dbeug`)
/// is a clear error rather than a silently-ignored filter directive.
fn validate_log_level(level: &str) -> Result<String, String> {
    match level {
        "off" | "error" | "warn" | "info" | "debug" | "trace" => Ok(level.to_string()),
        other => Err(format!(
            "--log-level: unknown level `{other}` (expected off|error|warn|info|debug|trace)"
        )),
    }
}

/// The default package-dir candidates appended AFTER any explicit
/// `--gems` dirs (explicit dirs get first-name-wins priority): the
/// input file's sibling `gems/` (project-local gems).
///
/// The compiler's OWN bundled `gems/` is not listed here: the loader appends
/// it unconditionally, so it is found whether zeo is driven through this
/// CLI or used as a library. The bundled path comes from the resolved home
/// (see `zeo::home`) -- the repo's `gems/` in the dev tree, the payload's
/// `gems/` next to an installed executable.
fn default_package_dirs(input: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = input.and_then(|p| p.parent()) {
        dirs.push(parent.join("gems"));
    }
    dirs
}

fn run() -> Result<(), MainError> {
    let mut args = match parse_args()? {
        Parsed::Run(args) => args,
        Parsed::Help => {
            print!("{HELP}");
            return Ok(());
        }
        Parsed::Version => {
            println!("zeo {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Parsed::NoInput => {
            print!("{HELP}");
            std::process::exit(1);
        }
    };
    init_tracing(args.log_level.as_deref());
    // Pre-flight: a broken install (payload missing next to the executable)
    // reports here as an ordinary error instead of panicking mid-compile.
    zeo::home::ensure_resolved()?;
    let (source, input_path) = match &args.source {
        Source::File(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            (text, Some(path.clone()))
        }
        Source::Eval(code) => (code.clone(), None),
    };

    let mut package_dirs = args.package_dirs.clone();
    package_dirs.extend(default_package_dirs(input_path.as_deref()));

    // A store asked for by FLAG must be usable; one assembled purely from
    // ambient env degrades to inactive when its lockfile is missing.
    if let Some(lock) = &args.lockfile
        && !lock.is_file()
    {
        if args.store_from_flags {
            return Err(
                format!("lockfile {} not found (run `bundle lock`?)", lock.display()).into(),
            );
        }
        args.gem_paths = Vec::new();
        args.lockfile = None;
    }

    let is_eval = matches!(args.source, Source::Eval(_));
    let gem_report = match &args.report {
        Report::Off => None,
        Report::Path(path) => Some(path.clone()),
        Report::DefaultPath => {
            let artifact = args.output.clone().unwrap_or_else(|| {
                let mut p = input_path.clone().expect("a file source always has a path");
                p.set_extension("");
                p
            });
            let dir = artifact
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            Some(dir.join("zeo-gems.json"))
        }
    };
    let opts = zeo::CompileOptions {
        input_path: input_path.clone(),
        load_roots: args.load_roots.clone(),
        package_dirs,
        // Warnings are the disclosure mechanism, so they are ALWAYS on at the
        // CLI -- independent of the opt-in report file. `-W0` (or a
        // `-W:no-<category>`) is the off switch. The library default stays
        // silent for in-process harness callers.
        gem_warnings: true,
        gem_report,
        nowarn: args.nowarn.clone(),
        gem_paths: args.gem_paths.clone(),
        lockfile: args.lockfile.clone(),
        pretty: args.dump_rust,
    };
    let compiled = zeo::compile_to_rust_with(&source, &opts)?;

    if args.dump_rust {
        println!("{}", compiled.rust_source);
        return Ok(());
    }

    use zeo::backend::{GenOpt, Linkage, Profile, Runtime, build_binary, ensure_runtime_built};

    // The CLI always produces a SELF-CONTAINED binary -- both the run-once `-e`
    // throwaway and the shipped `-o app` -- so it statically links the runtime.
    // Dynamic linkage (a smaller binary against a shared dylib) is the test
    // harness's concern, where the dylib always sits in the build tree; a CLI
    // artifact must not depend on that.
    let linkage = Linkage::Static;

    // Which runtime variant this program's binary links: the lean, parser-free
    // default, or the prism-linked `eval-vm` one iff the program reaches prism
    // at runtime. The single mapping point for both build paths below.
    let runtime = Runtime::for_prism(compiled.needs_prism_runtime);

    // `-e` without `-o`: compile to a throwaway binary, run it, and exit with
    // ITS status (stdout/stderr stream straight through) -- the
    // differential-harness path. Run-once, so it DEFAULTS to the fast-to-build
    // `Debug` runtime; a harness that compiles thousands of programs can flip
    // this to `Release` via `ZEO_RUNTIME_PROFILE` for a ~12x faster
    // per-program link. With `-o`, `-e` produces an artifact like the file
    // mode below instead of running.
    if (is_eval || args.run) && args.output.is_none() {
        let profile = Profile::from_env_or(Profile::Debug);
        ensure_runtime_built(profile, runtime, linkage)?;
        let bin = std::env::temp_dir().join(format!("zeo-e-{}", std::process::id()));
        build_binary(
            &compiled.rust_source,
            &bin,
            profile,
            runtime,
            linkage,
            GenOpt::Optimized,
        )?;
        let status = std::process::Command::new(&bin)
            .args(&args.program_args)
            .status()
            .map_err(|e| format!("running compiled program: {e}"))?;
        let _ = std::fs::remove_file(&bin);
        std::process::exit(status.code().unwrap_or(1));
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = match &args.source {
            Source::File(path) => path.clone(),
            Source::Eval(_) => unreachable!("-e without -o is handled above"),
        };
        p.set_extension("");
        p
    });
    // `zeo foo.rb -o app` produces a SHIPPED binary: DEFAULT to the
    // release-profiled runtime (optimized + stripped) so the artifact is small and
    // fast, rather than embedding the unoptimized debug runtime. One-time `cargo
    // build --release -p zeo-rt` on first use. Overridable to `debug` via
    // `ZEO_RUNTIME_PROFILE` (e.g. to symbolicate a runtime panic).
    let profile = Profile::from_env_or(Profile::Release);
    ensure_runtime_built(profile, runtime, linkage)?;
    Ok(build_binary(
        &compiled.rust_source,
        &output,
        profile,
        runtime,
        linkage,
        GenOpt::Optimized,
    )?)
}

/// Install a `tracing` subscriber (stderr) for the compiler pipeline. Sources,
/// highest precedence first: the `--log-level` flag (a bare level applied to the
/// `zeo` crate -- the front end's `zeo::hir`/`zeo::lower` spans included, now
/// that it is folded in), then `ZEO_LOG`, then `RUST_LOG` (both full
/// `EnvFilter` directives, for finer per-module control). With none of them set,
/// no subscriber is installed, so every `trace!`/`debug!`/`instrument` in the
/// pipeline compiles to a cheap disabled check -- a normal compile stays silent
/// and never interleaves with the miette diagnostics. Examples:
///   zeo prog.rb --log-level debug
///   ZEO_LOG=zeo::analyze=trace zeo prog.rb
fn init_tracing(log_level: Option<&str>) {
    let directive = match log_level {
        Some(level) => format!("zeo={level}"),
        None => match std::env::var("ZEO_LOG")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("RUST_LOG").ok().filter(|s| !s.is_empty()))
        {
            Some(directive) => directive,
            None => return,
        },
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(directive))
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

fn main() -> ExitCode {
    // A diagnostic read by a machine must not be reflowed. miette hard-wraps at
    // the terminal width and breaks after `/` and `-`, so an absolute path in a
    // message arrives split across two lines -- and whoever rejoins them cannot
    // tell the inserted space from a real one. That silently defeated the
    // gem-probe's path scrubber and committed 12 rows carrying this machine's
    // home directory. A pipe gets the message on one line; a terminal keeps the
    // wrapping, and colour still degrades on its own either way.
    if !std::io::stderr().is_terminal() {
        let _ = miette::set_hook(Box::new(|_| {
            Box::new(miette::MietteHandlerOpts::new().wrap_lines(false).build())
        }));
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(MainError::Plain(e)) => {
            eprintln!("zeo: {e}");
            ExitCode::FAILURE
        }
        Err(MainError::Compile(e)) => {
            // miette's report rendering: graphical with the source excerpt
            // on a terminal, degrading automatically when piped or under
            // NO_COLOR/TERM=dumb.
            eprintln!("{:?}", miette::Report::new(e));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_env(args: &[&str], env: &Env) -> Result<Parsed, String> {
        parse_args_from(args.iter().map(|s| s.to_string()).collect(), env)
    }

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_env(args, &Env::default())
    }

    /// A parse that must yield a runnable `Args`.
    fn ok(args: &[&str]) -> Args {
        match parse(args).expect("parse succeeds") {
            Parsed::Run(a) => *a,
            _ => panic!("expected Parsed::Run"),
        }
    }

    fn err(args: &[&str]) -> String {
        match parse(args) {
            Err(e) => e,
            Ok(_) => panic!("expected an error for {args:?}"),
        }
    }

    #[test]
    fn running_a_file_is_opt_in() {
        // zeo compiles code that has not necessarily been read yet, so naming
        // a file must never be enough to execute it.
        assert!(!ok(&["t.rb"]).run);
        assert!(ok(&["t.rb", "--run"]).run);
        assert!(ok(&["--run", "t.rb"]).run);
    }

    #[test]
    fn a_run_file_takes_argv_but_a_compiled_one_does_not() {
        let a = ok(&["t.rb", "--run", "alpha", "beta"]);
        assert_eq!(a.program_args, vec!["alpha", "beta"]);
        assert_eq!(
            ok(&["--run", "t.rb", "--", "-W0"]).program_args,
            vec!["-W0"]
        );
        // Without `--run` nothing runs, so there is no ARGV to take.
        assert!(err(&["t.rb", "alpha"]).contains("unexpected argument `alpha`"));
    }

    #[test]
    fn run_and_an_output_path_contradict() {
        assert!(err(&["t.rb", "--run", "-o", "app"]).contains("drop -o"));
    }

    #[test]
    fn gems_flag_collects_in_order_both_spellings() {
        let a = ok(&["--gems", "a", "--gems=b", "t.rb"]);
        assert_eq!(a.package_dirs, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn warning_categories_toggle_and_validate() {
        let a = ok(&["-W:no-zeo-builtin-substitute", "t.rb"]);
        assert!(a.nowarn.contains("zeo-builtin-substitute"));
        // The positive form re-enables -- last one wins.
        let a = ok(&[
            "-W:no-zeo-builtin-substitute",
            "-W:zeo-builtin-substitute",
            "t.rb",
        ]);
        assert!(a.nowarn.is_empty());
        let a = ok(&["-W0", "t.rb"]);
        assert!(a.nowarn.contains("zeo-builtin-substitute"));
        // Levels and -w are accepted no-ops.
        assert!(ok(&["-w", "-W", "-W1", "-W2", "t.rb"]).nowarn.is_empty());
        assert!(err(&["-W:no-typo", "t.rb"]).contains("unknown warning category"));
        assert!(err(&["-W3", "t.rb"]).contains("invalid option"));
    }

    #[test]
    fn unknown_flags_are_rejected_not_swallowed() {
        assert!(err(&["--bogus", "t.rb"]).contains("invalid option"));
        assert!(err(&["-q", "t.rb"]).contains("invalid option"));
    }

    #[test]
    fn removed_flags_point_at_their_replacement() {
        assert!(err(&["--packages", "d", "t.rb"]).contains("--gems"));
        assert!(err(&["--nowarn", "x", "t.rb"]).contains("-W:no-"));
        assert!(err(&["--nowarn=x", "t.rb"]).contains("-W:no-"));
        assert!(err(&["--lockfile", "l", "t.rb"]).contains("--bundle-gemfile"));
        assert!(err(&["--no-report", "t.rb"]).contains("--report"));
        assert!(err(&["-S", "t.rb"]).contains("--dump=rust"));
    }

    #[test]
    fn store_flags_pair_and_derive_the_lockfile() {
        assert!(err(&["--gem-path", "s", "t.rb"]).contains("--bundle-gemfile"));
        assert!(err(&["--bundle-gemfile", "G", "t.rb"]).contains("--gem-path"));
        let a = ok(&["--gem-path", "s", "--bundle-gemfile", "d/Gemfile", "t.rb"]);
        assert_eq!(a.gem_paths, vec![PathBuf::from("s")]);
        assert_eq!(a.lockfile, Some(PathBuf::from("d/Gemfile.lock")));
        assert!(a.store_from_flags);
        // A path that already IS a lockfile is taken as given; gems.rb maps
        // to bundler's gems.locked.
        let a = ok(&["--gem-path=s", "--bundle-gemfile=d/Gemfile.lock", "t.rb"]);
        assert_eq!(a.lockfile, Some(PathBuf::from("d/Gemfile.lock")));
        let a = ok(&["--gem-path=s", "--bundle-gemfile=d/gems.rb", "t.rb"]);
        assert_eq!(a.lockfile, Some(PathBuf::from("d/gems.locked")));
    }

    #[test]
    fn env_store_activates_only_as_a_pair() {
        let both = Env {
            gem_path: Some("s1:s2".into()),
            bundle_gemfile: Some("d/Gemfile".into()),
            ..Env::default()
        };
        let a = match parse_env(&["t.rb"], &both).unwrap() {
            Parsed::Run(a) => *a,
            _ => panic!(),
        };
        assert_eq!(a.gem_paths, vec![PathBuf::from("s1"), PathBuf::from("s2")]);
        assert_eq!(a.lockfile, Some(PathBuf::from("d/Gemfile.lock")));
        assert!(!a.store_from_flags);
        // Ambient GEM_PATH alone never activates the store.
        let alone = Env {
            gem_path: Some("s1".into()),
            ..Env::default()
        };
        let a = match parse_env(&["t.rb"], &alone).unwrap() {
            Parsed::Run(a) => *a,
            _ => panic!(),
        };
        assert!(a.gem_paths.is_empty());
        assert!(a.lockfile.is_none());
        // A flag completes an env half -- and counts as flag-driven.
        let a = match parse_env(&["--bundle-gemfile=G", "t.rb"], &alone).unwrap() {
            Parsed::Run(a) => *a,
            _ => panic!(),
        };
        assert_eq!(a.gem_paths, vec![PathBuf::from("s1")]);
        assert!(a.store_from_flags);
    }

    #[test]
    fn report_is_opt_in_and_only_the_attached_form_takes_a_path() {
        assert!(matches!(ok(&["t.rb"]).report, Report::Off));
        assert!(matches!(
            ok(&["t.rb", "--report"]).report,
            Report::DefaultPath
        ));
        match ok(&["t.rb", "--report=out.json"]).report {
            Report::Path(p) => assert_eq!(p, PathBuf::from("out.json")),
            _ => panic!("expected Report::Path"),
        }
        // `--report x.rb` must not eat x.rb as a value: x.rb is the input.
        let a = ok(&["--report", "x.rb"]);
        assert!(matches!(a.report, Report::DefaultPath));
        assert!(matches!(a.source, Source::File(ref p) if p == &PathBuf::from("x.rb")));
        // Bare --report has no artifact dir to land in under run-immediately -e.
        assert!(err(&["-e", "1", "--report"]).contains("--report=<path>"));
        assert!(parse(&["-e", "1", "-o", "bin", "--report"]).is_ok());
    }

    #[test]
    fn eval_mode_collects_argv_like_ruby() {
        let a = ok(&["-e", "p ARGV", "a", "-o", "b"]);
        // Option parsing stops at the first positional: -o is ARGV too.
        assert_eq!(a.program_args, vec!["a", "-o", "b"]);
        assert!(a.output.is_none());
        let a = ok(&["-e", "p ARGV", "--", "-x", "b"]);
        assert_eq!(a.program_args, vec!["-x", "b"]);
        // A compile (file mode, or -e with -o) has no ARGV to give.
        assert!(err(&["t.rb", "--", "x"]).contains("unexpected argument"));
        assert!(err(&["-e", "1", "-o", "bin", "x"]).contains("unexpected argument"));
    }

    #[test]
    fn dashdash_lets_a_dash_leading_filename_through() {
        let a = ok(&["--", "-weird.rb"]);
        assert!(matches!(a.source, Source::File(ref p) if p == &PathBuf::from("-weird.rb")));
    }

    #[test]
    fn rubyopt_is_restricted_and_the_command_line_wins() {
        let env = Env {
            rubyopt: Some("-W0 -Ia".to_string()),
            rubylib: Some("x:y".into()),
            ..Env::default()
        };
        let a = match parse_env(&["-Ib", "-W:zeo-builtin-substitute", "t.rb"], &env).unwrap() {
            Parsed::Run(a) => *a,
            _ => panic!(),
        };
        // CLI -W re-enables what RUBYOPT's -W0 suppressed.
        assert!(a.nowarn.is_empty());
        // Roots: CLI -I, then RUBYOPT -I, then RUBYLIB -- ruby's order.
        assert_eq!(
            a.load_roots,
            ["b", "a", "x", "y"].map(PathBuf::from).to_vec()
        );
        let bad = Env {
            rubyopt: Some("-e code".to_string()),
            ..Env::default()
        };
        match parse_env(&["t.rb"], &bad) {
            Err(e) => assert!(e.contains("illegal switch in RUBYOPT")),
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn informational_modes() {
        assert!(matches!(parse(&["--help"]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["-h"]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["--version"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(&["-v"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(&[]).unwrap(), Parsed::NoInput));
    }

    #[test]
    fn dump_replaces_dash_s() {
        assert!(ok(&["--dump=rust", "t.rb"]).dump_rust);
        assert!(err(&["--dump=insns", "t.rb"]).contains("unknown item"));
    }
}
