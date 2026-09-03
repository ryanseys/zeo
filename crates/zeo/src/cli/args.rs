//! The command line: every flag, and the shapes a parse can answer with.
//!
//! Hand-rolled, like the compiler's other front ends. The flags are ruby's
//! where ruby has one, so `--` and the file name end option parsing the same
//! way they do there.

use std::path::PathBuf;

use super::*;

pub(crate) struct Args {
    /// The input source: either a `.rb` file path or, with `-e`, an inline
    /// program string (`ruby -e`'s shape). Exactly one is required.
    pub(crate) source: Source,
    pub(crate) output: Option<PathBuf>,
    /// `-I` roots, then RUBYOPT's `-I` roots, then RUBYLIB -- ruby's order.
    pub(crate) load_roots: Vec<PathBuf>,
    /// `-r <lib>`: libraries required before the program's first line, in the
    /// order given (repeatable).
    pub(crate) required_libraries: Vec<String>,
    /// `--gems <dir>`: vendored-gem directories (repeatable).
    pub(crate) package_dirs: Vec<PathBuf>,
    /// `--embed-sources <dir>`: directories whose `.rb` files travel INSIDE
    /// the program, for a `require` only the run time can resolve
    /// (repeatable).
    pub(crate) embed_sources: Vec<PathBuf>,
    /// `--strict-static-require`: a `require`/`load` target the compiler
    /// cannot resolve is an error HERE, not at run time.
    pub(crate) strict_static_require: bool,
    /// `--package <feature>` -- compile the positional file as a separately
    /// linked package for that feature spelling; `-o` names the artifact
    /// (default: `<feature basename>.zeopkg` in the current directory).
    pub(crate) pkg_feature: Option<String>,
    /// `--with-package <artifact>` (repeatable) -- merge that package
    /// (a `.zeopkg` bundle, or an object with `<object>.zman` beside it)
    /// into this program.
    pub(crate) with_packages: Vec<PathBuf>,
    /// `--root-gem <name>`: the distinguished root package -- it outranks
    /// every other provider for an ambiguous feature (Bundler-root
    /// semantics). The gem probe names its subject here.
    pub(crate) root_gem: Option<String>,
    /// `--report[=<path>]`: the `zeo-gems.json` disclosure record, opt-in.
    pub(crate) report: Report,
    /// The external gem store dirs (`--gem-path`/`GEM_PATH`) and the lockfile
    /// derived from `--bundle-gemfile`/`BUNDLE_GEMFILE`. Either both are
    /// populated or neither -- `parse_args_from` enforces the pairing.
    pub(crate) gem_paths: Vec<PathBuf>,
    pub(crate) lockfile: Option<PathBuf>,
    /// Whether a store FLAG was given (vs env-only): flags demand a strict
    /// "lockfile must exist" check, ambient env degrades quietly.
    pub(crate) store_from_flags: bool,
    /// `--compile`: write the default-named binary (the input path with its
    /// extension stripped) instead of running.
    ///
    /// Running is the DEFAULT: a bare `zeo foo.rb` compiles and executes,
    /// exactly like `ruby foo.rb` (a deliberate reversal of the original
    /// opt-in-run decision -- ruby's mental model won). An artifact is what
    /// needs asking for now: `-o <path>` or this flag.
    pub(crate) compile: bool,
    /// ARGV for an immediately-run program (`-e`, or a file that runs):
    /// positionals and everything after `--`, exactly ruby's
    /// `[--] [args...]` shape.
    pub(crate) program_args: Vec<String>,
    /// `--emit-clif[=<path>]` / `--dump=clif`: emit the Cranelift IR instead
    /// of building -- to the attached path or stdout when bare. Implies the
    /// aot pipeline.
    pub(crate) emit_clif: Option<EmitTarget>,
    /// `--emit-zeodata=<path>`, with `--emit-clif`: write the `.zeodata`
    /// sidecar `zeo backend` reads beside the text.
    pub(crate) emit_zeodata: Option<PathBuf>,
    /// `--dump=syntax` (and `-c`): parse, print `Syntax OK`, and stop.
    pub(crate) check_syntax: bool,
    /// `--dump=units` / `--dump=classes[=<filter>]`: run the front end,
    /// print the named report, and stop.
    pub(crate) dump_front_end: Option<FrontEndDump>,
    /// `--backend <aot|jit>`: which Cranelift mode builds the program
    /// (`ZEO_BACKEND` is the env spelling; the flag wins). `None` = the
    /// default for the mode, which `Backend::select` decides.
    pub(crate) backend: Option<crate::backend::Backend>,
    /// `-g`: put DWARF line tables in the emitted object, so a native
    /// debugger or profiler renders a compiled frame as `file.rb:line`.
    /// `ZEO_DEBUGINFO=1` is the env spelling.
    pub(crate) debuginfo: bool,
    /// `--log-level <level|directive>`: the compiler's own `tracing` output.
    /// A bare level widens to every zeo crate; anything with a `=` or a `,`
    /// is passed to `EnvFilter` as written. Wins over `ZEO_LOG`/`RUST_LOG`,
    /// because a flag is more specific than an ambient variable.
    pub(crate) log_level: Option<String>,
    /// `--link <arg>` (repeatable): extra arguments for the `cc` link line,
    /// verbatim and in order. `ZEO_LINK_ARGS` (whitespace-split) is the env
    /// spelling; its arguments come first. Only a linked artifact reads them.
    pub(crate) link_args: Vec<String>,
}

/// A `--dump` kind the FRONT END answers: no emission, no artifact, no
/// backend -- see `crate::dump`.
pub(crate) enum FrontEndDump {
    Units,
    /// `--dump=classes` bare, or `--dump=classes=Bundler::Source` to keep
    /// only the classes whose name contains that text. A program that loads
    /// bundler defines 1,600 classes, so unfiltered is rarely what is
    /// wanted.
    Classes(Option<String>),
    /// `--dump=methods` bare shows the 20 widest definitions, or
    /// `--dump=methods=100` that many. The tally at the end is the part that
    /// matters; the rows are there to say WHERE the width comes from.
    Methods(usize),
}

/// Where `--emit-clif` sends the emitted IR. The path is ATTACHED-only
/// (`--emit-clif=out.clif`) -- a spaced value would be ambiguous with the
/// input file, the same reason `--report` is attached-only.
pub(crate) enum EmitTarget {
    Stdout,
    File(PathBuf),
}

pub(crate) enum Source {
    File(PathBuf),
    /// `-e <code>` (repeatable; joined with newlines, like `ruby -e`). Compiles
    /// AND runs immediately, forwarding stdout/stderr and the exit status --
    /// the shape the `ruby`-differential harness drives.
    Eval(String),
    /// Bare `zeo` on an interactive terminal: the irb shell.
    Irb,
}

/// The program a bare `zeo` runs. `IRB.start` reads the terminal itself, so
/// this is the whole of it -- the same two lines irb's own binstub writes.
pub(crate) const IRB_DRIVER: &str = "require \"irb\"\nIRB.start\n";

/// The `zeo-gems.json` disclosure record: off unless `--report` asked for it.
pub(crate) enum Report {
    Off,
    /// Bare `--report`: next to the output artifact.
    DefaultPath,
    /// `--report=<path>`.
    Path(PathBuf),
}

/// `parse_args_from`'s outcome: a compile to run, or an informational mode
/// the wrapper prints and exits for.
pub(crate) enum Parsed {
    Run(Box<Args>),
    /// `zeo install`: precompile the project's locked gems into the store.
    Install(InstallCmd),
    /// `zeo flags`: print the compile flags the project implies.
    Flags(FlagsCmd),
    /// `zeo backend`: link a program from CLIF text another front end wrote.
    Backend(BackendCmd),
    /// `zeo gem precompile`: build this gem's platform gem, artifact inside.
    GemPrecompile,
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
pub(crate) struct Env {
    pub(crate) rubyopt: Option<String>,
    pub(crate) rubylib: Option<std::ffi::OsString>,
    pub(crate) gem_path: Option<std::ffi::OsString>,
    pub(crate) bundle_gemfile: Option<std::ffi::OsString>,
}

impl Env {
    pub(crate) fn from_process() -> Env {
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

pub(crate) fn parse_args() -> Result<Parsed, String> {
    parse_args_from(std::env::args().skip(1).collect(), &Env::from_process())
}

pub(crate) fn parse_args_from(argv: Vec<String>, env: &Env) -> Result<Parsed, String> {
    // `zeo build <file.rb>`: the verb form of `--compile`. Unlike a run,
    // a build has no program ARGV, so option parsing does NOT stop at the
    // file -- `zeo build app.rb -o dist/app` reads naturally. The name is
    // fixed like the other subcommands: a script really called `build`
    // still runs as `zeo ./build`.
    let build_verb = argv.first().map(String::as_str) == Some("build");
    let argv: Vec<String> = if build_verb {
        argv.into_iter().skip(1).collect()
    } else {
        argv
    };
    // `zeo install`: zeo's own verb, with its own small flag set -- it is
    // not a program run, so none of ruby's parsing rules apply to it.
    if !build_verb && argv.first().map(String::as_str) == Some("install") {
        return parse_install(&argv[1..]).map(Parsed::Install);
    }
    // `zeo flags`: likewise zeo's own verb.
    if !build_verb && argv.first().map(String::as_str) == Some("flags") {
        return parse_flags(&argv[1..]).map(Parsed::Flags);
    }
    // `zeo backend`: CLIF text in, a binary out; no Ruby is read.
    if !build_verb && argv.first().map(String::as_str) == Some("backend") {
        return parse_backend(&argv[1..]).map(Parsed::Backend);
    }
    // `zeo gem precompile` is zeo's, not a rubygems command: it is caught
    // here, before the `gem` rewrite hands everything to `Gem::GemRunner`.
    if !build_verb
        && argv.first().map(String::as_str) == Some("gem")
        && argv.get(1).map(String::as_str) == Some("precompile")
    {
        if argv.len() > 2 {
            return Err(
                "zeo gem precompile takes no arguments; run it from the gem's own directory"
                    .to_string(),
            );
        }
        return Ok(Parsed::GemPrecompile);
    }
    // A subcommand becomes `-e <driver> -- <its own arguments>`, and the `--`
    // is what keeps them its own: without it a `zeo gem --version` would read
    // as zeo's `--version` rather than rubygems'.
    let argv = match subcommand_driver(argv.first().map(String::as_str)) {
        Some((driver, keeps_name)) => std::iter::once("-e".to_string())
            .chain(std::iter::once(driver.to_string()))
            .chain(std::iter::once("--".to_string()))
            .chain(argv.into_iter().skip(usize::from(!keeps_name)))
            .collect(),
        None => argv,
    };
    let mut input = None;
    let mut eval: Option<String> = None;
    let mut output = None;
    let mut emit_clif: Option<EmitTarget> = None;
    let mut emit_zeodata: Option<PathBuf> = None;
    let mut check_syntax = false;
    let mut dump_front_end: Option<FrontEndDump> = None;
    // The env spelling is read once here so the flag and the variable can
    // never disagree downstream.
    let mut debuginfo = std::env::var_os("ZEO_DEBUGINFO").is_some_and(|v| v != "0");
    // Likewise: the env spelling seeds the list and every `--link` appends.
    let mut link_args: Vec<String> = std::env::var("ZEO_LINK_ARGS")
        .map(|v| v.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let mut log_level: Option<String> = None;
    let mut load_roots = Vec::new();
    let mut package_dirs = Vec::new();
    let mut embed_sources = Vec::new();
    let mut strict_static_require = false;
    let mut pkg_feature: Option<String> = None;
    let mut with_packages: Vec<PathBuf> = Vec::new();
    let mut root_gem: Option<String> = None;
    let mut report = Report::Off;
    let mut gem_paths: Vec<PathBuf> = Vec::new();
    let mut gemfile: Option<PathBuf> = None;
    let mut compile = false;
    // `--irb`: the shell a bare `zeo` opens on a terminal, asked for by name
    // so it works where there is no terminal to detect -- a wrapper script, a
    // pty a test drives, an editor's run pane.
    let mut irb = false;
    let mut backend: Option<crate::backend::Backend> = None;
    let mut program_args: Vec<String> = Vec::new();
    let mut required_libraries: Vec<String> = Vec::new();
    let mut features = crate::cli::features::RubyFeatures::default();

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
                // ARGV for a run file; rejected below if nothing runs.
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
            // ruby's two spellings for the same dial: `--disable=gems` and
            // `--disable-gems`. Both reach the same table.
            let feature_switch = match (name, &inline) {
                ("enable", Some(list)) => Some((list.clone(), true)),
                ("disable", Some(list)) => Some((list.clone(), false)),
                _ => name
                    .strip_prefix("enable-")
                    .map(|f| (f.to_string(), true))
                    .or_else(|| {
                        name.strip_prefix("disable-")
                            .map(|f| (f.to_string(), false))
                    }),
            };
            if let Some((list, on)) = feature_switch {
                features.set(&list, on)?;
                continue;
            }
            match name {
                "help" => return Ok(Parsed::Help),
                "version" => return Ok(Parsed::Version),
                "enable" | "disable" => {
                    return Err(format!("--{name} needs a feature, e.g. --{name}=gems"));
                }
                "compile" => compile = true,
                "irb" => irb = true,
                "backend" => backend = Some(crate::backend::Backend::parse(&value("--backend")?)?),
                "gems" => package_dirs.push(PathBuf::from(value("--gems")?)),
                "package" => {
                    pkg_feature = Some(value("--package")?);
                }
                "with-package" => {
                    with_packages.push(PathBuf::from(value("--with-package")?));
                }
                "link" => link_args.push(value("--link")?),
                "embed-sources" => {
                    embed_sources.push(PathBuf::from(value("--embed-sources")?));
                }
                "log-level" => log_level = Some(value("--log-level")?),
                "strict-static-require" => strict_static_require = true,
                "root-gem" => root_gem = Some(value("--root-gem")?),
                "gem-path" => gem_paths.push(PathBuf::from(value("--gem-path")?)),
                "bundle-gemfile" => {
                    gemfile = Some(PathBuf::from(value("--bundle-gemfile")?));
                }
                // Only the attached form takes a path -- a spaced value would
                // be ambiguous with the input file.
                "report" => {
                    report = match inline {
                        Some(path) => Report::Path(PathBuf::from(path)),
                        None => Report::DefaultPath,
                    };
                }
                // Attached path only, like --report -- a spaced value would
                // be ambiguous with the input file. Bare means stdout.
                "emit-clif" => {
                    emit_clif = Some(match inline {
                        Some(path) => EmitTarget::File(PathBuf::from(path)),
                        None => EmitTarget::Stdout,
                    });
                }
                "emit-zeodata" => {
                    emit_zeodata =
                        Some(PathBuf::from(inline.ok_or(
                            "--emit-zeodata takes an attached path (--emit-zeodata=<path>)",
                        )?));
                }
                // ruby's own spelling for the same family. Where CRuby WARNS
                // for a kind it cannot serve and keeps running, zeo errors:
                // running while printing nothing is the silent drop the
                // project forbids, and the CLI already refuses flags on
                // purpose (`-S`, `--nowarn`).
                "dump" => match inline
                    .as_deref()
                    .map(|k| k.split_once('=').unwrap_or((k, "")))
                {
                    Some(("clif", "")) => emit_clif = Some(EmitTarget::Stdout),
                    Some(("syntax", "")) => check_syntax = true,
                    // zeo's own two, beside `clif`. `units` is the compiled-in
                    // load path a program ends up with; `classes` is what
                    // decides whether each class's constant resolves. Both
                    // answer a question a loading failure raises, and reading
                    // either off a 55-second probe was how three wrong
                    // diagnoses in a row got written.
                    Some(("units", "")) => dump_front_end = Some(FrontEndDump::Units),
                    Some(("classes", filter)) => {
                        dump_front_end = Some(FrontEndDump::Classes(
                            (!filter.is_empty()).then(|| filter.to_string()),
                        ));
                    }
                    Some(("methods", count)) => {
                        let top = match count.is_empty() {
                            true => 20,
                            false => count.parse().map_err(|_| {
                                format!("--dump=methods={count} wants a row count, e.g. 100")
                            })?,
                        };
                        dump_front_end = Some(FrontEndDump::Methods(top));
                    }
                    Some(("insns", _)) => {
                        return Err("--dump=insns has no answer here: zeo compiles ahead of \
                                    time and emits no bytecode (--dump=clif shows the IR it \
                                    does emit)"
                            .to_string());
                    }
                    Some(("parsetree", _)) => {
                        return Err("--dump=parsetree is not implemented: ruby renders prism's \
                                    tree from its Ruby side, and zeo parses through the Rust \
                                    bindings, which carry no printer"
                            .to_string());
                    }
                    Some((other, _)) => {
                        return Err(format!(
                            "--dump={other} is not a dump zeo knows \
                             (clif, syntax, units, classes, methods; insns and parsetree are refused)"
                        ));
                    }
                    None => return Err("--dump needs a kind, e.g. --dump=clif".to_string()),
                },
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
                // Require a library before the program's first line, like
                // ruby's own -r (repeatable, in the order given).
                "-r" => {
                    required_libraries.push(iter.next().ok_or("-r requires a library name")?);
                }
                "-g" => debuginfo = true,
                // ruby's short spelling of `--dump=syntax`.
                "-c" => check_syntax = true,
                "-h" => return Ok(Parsed::Help),
                "-v" => return Ok(Parsed::Version),
                _ => {
                    // Attached `-I<dir>` (ruby's own spelling, no space);
                    // `-I=<dir>` is accepted too, matching the long options'
                    // attached-equals convention.
                    if let Some(dir) = arg
                        .strip_prefix("-I")
                        .map(|d| d.strip_prefix('=').unwrap_or(d))
                        .filter(|d| !d.is_empty())
                    {
                        load_roots.push(PathBuf::from(dir));
                    } else if let Some(lib) = arg
                        .strip_prefix("-r")
                        .map(|l| l.strip_prefix('=').unwrap_or(l))
                        .filter(|l| !l.is_empty())
                    {
                        // Attached `-rjson`, ruby's own spelling.
                        required_libraries.push(lib.to_string());
                    } else if arg == "-w" || arg.starts_with("-W") {
                        warn_flag(&arg)?;
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
            // Option parsing STOPS at the script name -- ruby's own rule, and
            // the only one under which a program can have flags of its own.
            // `ruby test.rb --seed 42 -v` hands all four to ARGV, and zeo has
            // to as well: `--seed`, `--verbose`, `-n`, `--format` are how
            // every test framework is driven, and reading them as zeo's own
            // made `zeo test.rb --seed 42` an "invalid option" error.
            //
            // Options still come BEFORE the file (`zeo -o bin test.rb`), which
            // is where ruby wants them too.
            //
            // A build has no program ARGV, so its options may follow the file
            // (`zeo build app.rb -o dist/app`); a trailing positional is
            // still rejected below, since nothing runs to receive it.
            collecting_argv = !build_verb;
        } else {
            program_args.push(arg);
        }
    }

    // RUBYOPT is read AFTER the command line, because `--disable=rubyopt` is
    // on the command line and has to be able to turn it off. Its roots still
    // sit behind the `-I` roots in the final order, which is ruby's
    // precedence: the command line wins wherever both touch one dial.
    // Only the option subset that can't smuggle in a program is allowed.
    let mut rubyopt_roots = Vec::new();
    if let Some(opt) = env.rubyopt.as_ref().filter(|_| features.is_on("rubyopt")) {
        let mut toks = opt.split_whitespace();
        while let Some(tok) = toks.next() {
            if tok == "-I" {
                let dir = toks.next().ok_or("-I in RUBYOPT requires a directory")?;
                rubyopt_roots.push(PathBuf::from(dir));
            } else if let Some(dir) = tok
                .strip_prefix("-I")
                .map(|d| d.strip_prefix('=').unwrap_or(d))
                .filter(|d| !d.is_empty())
            {
                rubyopt_roots.push(PathBuf::from(dir));
            } else if tok == "-w" || tok.starts_with("-W") {
                warn_flag(tok)?;
            } else {
                return Err(format!("illegal switch in RUBYOPT: {tok}"));
            }
        }
    }

    // What the build (and `--enable=`) put in front of the program, ahead of
    // anything `-r` named: RubyGems has to be there before a `-r` of a gem
    // can find it.
    let mut required_libraries = {
        let mut all = features.ambient_libraries();
        all.extend(required_libraries);
        all
    };
    required_libraries.dedup();

    // The verb is `--compile` by another name, and it wants a file: a build
    // names its binary after the input, which `-e` and a shell lack.
    if build_verb {
        if input.is_none() {
            return Err("zeo build needs a file to compile, e.g. `zeo build app.rb`".to_string());
        }
        compile = true;
    }
    // A package artifact has a natural default name; `-o` still wins.
    if pkg_feature.is_some() && output.is_none() {
        let base = pkg_feature
            .as_deref()
            .and_then(|f| f.rsplit('/').next())
            .expect("pkg_feature checked some above");
        output = Some(PathBuf::from(format!("{base}.zeopkg")));
    }
    let source = match (eval, input) {
        (Some(_), Some(_)) => return Err("cannot combine -e with a file argument".to_string()),
        (Some(code), None) => Source::Eval(code),
        (None, Some(path)) => Source::File(path),
        // Bare `zeo` opens a shell when there is a terminal to talk to. Ruby
        // reads a program from stdin instead, so the test is both ends of the
        // pipe: a piped or redirected `zeo` behaves as it did.
        //
        // Only when nothing else was asked for: `zeo -o out` names an
        // artifact and has no program to put in it, which stays the error it
        // has always been rather than becoming a shell.
        // Asked for by NAME. It answers before the terminal test, so
        // `zeo --irb < script` opens a shell rather than reading the script.
        (None, None) if irb => Source::Irb,
        (None, None)
            if interactive_terminal()
                && output.is_none()
                && !compile
                && emit_clif.is_none()
                && !check_syntax
                && dump_front_end.is_none() =>
        {
            Source::Irb
        }
        (None, None) => return Ok(Parsed::NoInput),
    };
    // `--compile` names the binary after the input file, which `-e` lacks.
    if compile && matches!(source, Source::Eval(_)) {
        return Err("--compile with -e has no input filename to name the binary; use -o".into());
    }
    // Both inspect instead of building, so neither has an artifact to name
    // or a backend to pick -- and honouring one silently while ignoring the
    // other is exactly the silent drop this CLI refuses.
    if [emit_clif.is_some(), check_syntax, dump_front_end.is_some()]
        .iter()
        .filter(|on| **on)
        .count()
        > 1
    {
        return Err("each --dump kind stops before the others run; ask for one".to_string());
    }
    if emit_zeodata.is_some() && emit_clif.is_none() {
        return Err(
            "--emit-zeodata goes with --emit-clif, which writes the text it describes".into(),
        );
    }
    for (flag, set) in [
        ("-o", output.is_some()),
        ("--compile", compile),
        ("--backend", backend.is_some()),
        ("-g", debuginfo),
        ("--link", !link_args.is_empty()),
    ] {
        if !set {
            continue;
        }
        if emit_clif.is_some() {
            return Err(format!(
                "--dump=clif inspects instead of building, so {flag} would be ignored"
            ));
        }
        if check_syntax {
            return Err(format!(
                "--dump=syntax stops after parsing, so {flag} would be ignored"
            ));
        }
        if dump_front_end.is_some() {
            return Err(format!(
                "a front-end --dump stops before emission, so {flag} would be ignored"
            ));
        }
    }
    // Trailing args are ARGV, which only an immediately-run program has.
    // (Both dump kinds inspect instead of running, so neither has any.)
    let runs_now = output.is_none()
        && !compile
        && emit_clif.is_none()
        && !check_syntax
        && dump_front_end.is_none();
    if !program_args.is_empty() && !runs_now {
        return Err(format!("unexpected argument `{}`", program_args[0]));
    }
    // Bare `--report` derives its path from the output artifact, which an
    // immediately-run program does not have.
    if matches!(report, Report::DefaultPath) && output.is_none() && !compile {
        return Err(
            "--report without an artifact has nowhere to land; use --report=<path>, -o \
             or --compile"
                .to_string(),
        );
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
        embed_sources,
        strict_static_require,
        pkg_feature,
        with_packages,
        source,
        output,
        compile,
        required_libraries,
        load_roots: {
            let mut roots = load_roots;
            roots.extend(rubyopt_roots);
            if let Some(lib) = &env.rubylib {
                roots.extend(std::env::split_paths(lib));
            }
            roots
        },
        package_dirs,
        root_gem,
        report,
        store_from_flags: gem_paths_from_flag || gemfile_from_flag,
        gem_paths,
        lockfile: gemfile.map(crate::gems::project::derive_lockfile),
        program_args,
        emit_clif,
        emit_zeodata,
        check_syntax,
        dump_front_end,
        debuginfo,
        backend,
        log_level,
        link_args,
    })))
}

/// Every `-W:[no-]<category>` ruby 4.0.6 accepts. zeo emits none of these
/// categories, so toggling one is a no-op -- but an unknown name is still
/// reported, exactly as ruby reports it, so a typo is not silently ignored.
pub(crate) const WARNING_CATEGORIES: &[&str] = &[
    "deprecated",
    "experimental",
    "performance",
    "strict_unused_block",
];

/// The `-w`/`-W` family, ruby's shapes. Every form is accepted and none
/// changes what zeo prints: the compiler's own diagnostics are errors, and
/// the warning categories above belong to a runtime zeo does not warn from.
/// An unknown category warns and continues, as ruby's own driver does.
pub(crate) fn warn_flag(arg: &str) -> Result<(), String> {
    match arg {
        "-w" | "-W" | "-W0" | "-W1" | "-W2" => Ok(()),
        _ => match arg.strip_prefix("-W:") {
            Some(category) => {
                let name = category.strip_prefix("no-").unwrap_or(category);
                if !WARNING_CATEGORIES.contains(&name) {
                    eprintln!("zeo: warning: unknown warning category: '{name}'");
                }
                Ok(())
            }
            None => Err(format!(
                "invalid option: {arg} (-h will show valid options)"
            )),
        },
    }
}

/// The default package-dir candidates appended AFTER any explicit
/// `--gems` dirs (explicit dirs get first-name-wins priority): the
/// input file's sibling `gems/` (project-local gems).
///
/// The compiler's OWN libraries are not listed here: the loader appends them
/// unconditionally, so they are found whether zeo is driven through this CLI
/// or used as a library. They come from the resolved home (see `crate::home`);
/// `crate::gems::bundled` decides the set.
pub(crate) fn default_package_dirs(input: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = input.and_then(|p| p.parent()) {
        dirs.push(parent.join("gems"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn parse_env(args: &[&str], env: &Env) -> Result<Parsed, String> {
        parse_args_from(args.iter().map(|s| s.to_string()).collect(), env)
    }

    pub(crate) fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_env(args, &Env::default())
    }

    /// A parse that must yield a runnable `Args`.
    pub(crate) fn ok(args: &[&str]) -> Args {
        match parse(args).expect("parse succeeds") {
            Parsed::Run(a) => *a,
            _ => panic!("expected Parsed::Run"),
        }
    }

    pub(crate) fn err(args: &[&str]) -> String {
        match parse(args) {
            Err(e) => e,
            Ok(_) => panic!("expected an error for {args:?}"),
        }
    }

    /// `zeo build` is `--compile` as a verb, and because a build has no
    /// program ARGV its options may follow the file.
    #[test]
    pub(crate) fn the_build_verb_compiles_without_running() {
        let a = ok(&["build", "app.rb"]);
        assert!(a.compile);
        assert!(matches!(&a.source, Source::File(p) if p == std::path::Path::new("app.rb")));

        let a = ok(&["build", "app.rb", "-o", "dist/app"]);
        assert!(a.compile);
        assert_eq!(a.output.as_deref(), Some(std::path::Path::new("dist/app")));

        assert!(err(&["build"]).contains("needs a file"));
        assert!(err(&["build", "-e", "1"]).contains("needs a file"));
        // Nothing runs, so a trailing positional has no ARGV to join.
        assert!(err(&["build", "app.rb", "extra"]).contains("unexpected argument"));
        // The fixed-name rule: a script called `build` runs via a path.
        assert!(matches!(
            &ok(&["./build"]).source,
            Source::File(p) if p == std::path::Path::new("./build")
        ));
    }

    /// `--package` emits an artifact, so it defaults its own output name;
    /// `--with-package` is repeatable.
    #[test]
    pub(crate) fn package_flags_parse_with_a_default_artifact_name() {
        let a = ok(&["build", "--package", "rack", "entry.rb"]);
        assert_eq!(a.pkg_feature.as_deref(), Some("rack"));
        assert_eq!(
            a.output.as_deref(),
            Some(std::path::Path::new("rack.zeopkg"))
        );

        // A nested feature names the artifact by its basename.
        let a = ok(&["--package", "rack/utils", "-o", "x.zeopkg", "entry.rb"]);
        assert_eq!(a.output.as_deref(), Some(std::path::Path::new("x.zeopkg")));
        let a = ok(&["--package", "rack/utils", "entry.rb"]);
        assert_eq!(
            a.output.as_deref(),
            Some(std::path::Path::new("utils.zeopkg"))
        );

        let a = ok(&[
            "--with-package",
            "a.zeopkg",
            "--with-package",
            "b.zeopkg",
            "app.rb",
        ]);
        assert_eq!(a.with_packages.len(), 2);
    }

    /// `zeo gem` / `zeo bundle` run the vendored library, and every argument
    /// after the verb belongs to IT.
    ///
    /// The `--` the rewrite inserts is what makes that true. Without it a
    /// `zeo gem --version` would print zeo's version instead of rubygems',
    /// and `zeo bundle --help` would print zeo's help -- both of which are
    /// wrong in the quiet way, answering a plausible thing to the wrong
    /// question.
    #[test]
    pub(crate) fn a_subcommand_runs_the_vendored_library_and_keeps_its_own_flags() {
        let gem = ok(&["gem", "install", "--no-document", "rack"]);
        match &gem.source {
            Source::Eval(code) => assert!(code.contains("Gem::GemRunner")),
            _ => panic!("expected an eval source"),
        }
        assert_eq!(gem.program_args, ["install", "--no-document", "rack"]);

        // A flag that zeo also has still reaches rubygems.
        assert_eq!(ok(&["gem", "--version"]).program_args, ["--version"]);
        assert_eq!(ok(&["bundle", "--help"]).program_args, ["--help"]);
        assert_eq!(ok(&["gem"]).program_args, [] as [String; 0]);

        match &ok(&["bundle", "install"]).source {
            Source::Eval(code) => assert!(code.contains("Bundler::CLI.start")),
            _ => panic!("expected an eval source"),
        }
        // `bundler` is the same verb, which is what the binstub is called on
        // some installs.
        assert!(matches!(ok(&["bundler", "-v"]).source, Source::Eval(_)));

        // `zeo install` is zeo's OWN verb now (precompile the project's
        // gems); Bundler's install is spelled `zeo bundle install`.
        match parse(&["install", "rack", "--gem-path", "/s"]).expect("parses") {
            Parsed::Install(cmd) => {
                assert_eq!(cmd.names, ["rack"]);
                assert_eq!(cmd.gem_paths, [PathBuf::from("/s")]);
            }
            _ => panic!("expected Parsed::Install"),
        }
        assert!(
            err(&["install", "--local"]).contains("invalid option for zeo install"),
            "a Bundler flag no longer reaches Bundler through this verb"
        );

        // `zeo backend`: one CLIF file, an optional sidecar, an output.
        match parse(&["backend", "p.clif", "--data", "p.zeodata", "-o", "p"]).expect("parses") {
            Parsed::Backend(cmd) => {
                assert_eq!(cmd.clif, PathBuf::from("p.clif"));
                assert_eq!(cmd.data, Some(PathBuf::from("p.zeodata")));
                assert_eq!(cmd.output, PathBuf::from("p"));
            }
            _ => panic!("expected Parsed::Backend"),
        }
        assert!(err(&["backend", "p.clif"]).contains("-o"));
        assert!(err(&["backend", "-o", "p"]).contains("CLIF file"));
        assert!(err(&["backend", "a.clif", "b.clif", "-o", "p"]).contains("a second"));
        assert!(err(&["--emit-zeodata=p.zeodata", "t.rb"]).contains("--emit-clif"));
        assert!(err(&["--emit-clif", "--emit-zeodata", "t.rb"]).contains("attached path"));

        // The verb is only a verb in FIRST position. A file really called
        // `gem` is reachable, and a file whose name merely contains it is
        // untouched.
        assert!(matches!(ok(&["./gem"]).source, Source::File(_)));
        assert!(matches!(ok(&["gemfile.rb"]).source, Source::File(_)));
        assert!(matches!(ok(&["-e", "1", "gem"]).source, Source::Eval(_)));
        assert_eq!(ok(&["-e", "1", "gem"]).program_args, ["gem"]);
    }

    #[test]
    pub(crate) fn a_bare_file_runs_and_an_artifact_needs_asking_for() {
        // ruby's mental model: naming a file runs it. An artifact is the
        // opt-in now -- `-o <path>`, or `--compile` for the default name.
        let a = ok(&["t.rb"]);
        assert!(!a.compile && a.output.is_none());
        assert!(ok(&["--compile", "t.rb"]).compile);
        assert!(ok(&["--compile", "t.rb"]).compile);
        // --compile can't name a binary for -e.
        assert!(err(&["-e", "1", "--compile"]).contains("use -o"));
        // The old opt-in spelling is gone with the rest of the dead flags.
        // BEFORE the file name, where zeo's own options live -- after it,
        // `--run` would be the program's ARGV.
        assert!(err(&["--run", "t.rb"]).contains("invalid option"));
    }

    #[test]
    pub(crate) fn a_run_file_takes_argv_but_a_compiled_one_does_not() {
        let a = ok(&["t.rb", "alpha", "beta"]);
        assert_eq!(a.program_args, vec!["alpha", "beta"]);
        // Ruby-verified: once the script name is seen, `--` is ARGV like
        // anything else (`ruby t.rb -- -W0` gives `["--", "-W0"]`). It is only
        // an option terminator BEFORE a script name, which is where `-e` uses
        // it.
        assert_eq!(ok(&["t.rb", "--", "-W0"]).program_args, vec!["--", "-W0"]);
        // An artifact or inspect mode runs nothing, so there is no ARGV.
        assert!(err(&["--compile", "t.rb", "alpha"]).contains("unexpected argument `alpha`"));
        assert!(err(&["-o", "app", "t.rb", "alpha"]).contains("unexpected argument `alpha`"));
        assert!(err(&["--emit-clif", "t.rb", "alpha"]).contains("unexpected argument `alpha`"));
        // ...and zeo's own flags after the file name are the PROGRAM's, which
        // is the whole point: `--compile` here is ARGV, so the file RUNS.
        let a = ok(&["t.rb", "--compile", "-o", "app"]);
        assert!(!a.compile && a.output.is_none());
        assert_eq!(a.program_args, vec!["--compile", "-o", "app"]);
    }

    #[test]
    pub(crate) fn gems_flag_collects_in_order_both_spellings() {
        let a = ok(&["--gems", "a", "--gems=b", "t.rb"]);
        assert_eq!(a.package_dirs, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    pub(crate) fn warning_flags_are_accepted_no_ops() {
        // Every ruby shape parses and leaves the compile unchanged.
        for arg in [
            "-w",
            "-W",
            "-W0",
            "-W1",
            "-W2",
            "-W:deprecated",
            "-W:no-experimental",
            "-W:no-performance",
            "-W:no-strict_unused_block",
            // An unknown category warns on stderr, as ruby's driver does,
            // and still runs.
            "-W:no-typo",
        ] {
            let a = ok(&[arg, "t.rb"]);
            assert!(
                matches!(a.source, Source::File(ref p) if p == &PathBuf::from("t.rb")),
                "{arg}"
            );
        }
        assert!(err(&["-W3", "t.rb"]).contains("invalid option"));
    }

    #[test]
    pub(crate) fn unknown_flags_are_rejected_not_swallowed() {
        assert!(err(&["--bogus", "t.rb"]).contains("invalid option"));
        assert!(err(&["-q", "t.rb"]).contains("invalid option"));
    }

    #[test]
    pub(crate) fn dead_flag_spellings_get_the_generic_rejection() {
        // The pointed migration errors served their year; old spellings now
        // fail like any other unknown option.
        for old in ["--packages", "--nowarn", "--lockfile", "--no-report"] {
            assert!(err(&[old, "t.rb"]).contains("invalid option"), "{old}");
        }
        assert!(err(&["-S", "t.rb"]).contains("invalid option"));
    }

    #[test]
    pub(crate) fn store_flags_pair_and_derive_the_lockfile() {
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
    pub(crate) fn env_store_activates_only_as_a_pair() {
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
    pub(crate) fn report_is_opt_in_and_only_the_attached_form_takes_a_path() {
        assert!(matches!(ok(&["t.rb"]).report, Report::Off));
        assert!(matches!(
            ok(&["--compile", "--report", "t.rb"]).report,
            Report::DefaultPath
        ));
        match ok(&["--report=out.json", "t.rb"]).report {
            Report::Path(p) => assert_eq!(p, PathBuf::from("out.json")),
            _ => panic!("expected Report::Path"),
        }
        // `--report x.rb` must not eat x.rb as a value: x.rb is the input.
        let a = ok(&["--report", "--compile", "x.rb"]);
        assert!(matches!(a.report, Report::DefaultPath));
        assert!(matches!(a.source, Source::File(ref p) if p == &PathBuf::from("x.rb")));
        // Bare --report has no artifact dir to land in when the program runs
        // immediately instead of leaving a binary behind.
        assert!(err(&["-e", "1", "--report"]).contains("--report=<path>"));
        assert!(err(&["--report", "t.rb"]).contains("--report=<path>"));
        assert!(parse(&["-e", "1", "-o", "bin", "--report"]).is_ok());
    }

    #[test]
    pub(crate) fn eval_mode_collects_argv_like_ruby() {
        let a = ok(&["-e", "p ARGV", "a", "-o", "b"]);
        // Option parsing stops at the first positional: -o is ARGV too.
        assert_eq!(a.program_args, vec!["a", "-o", "b"]);
        assert!(a.output.is_none());
        let a = ok(&["-e", "p ARGV", "--", "-x", "b"]);
        assert_eq!(a.program_args, vec!["-x", "b"]);
        // In FILE mode the script name already stopped option parsing, so
        // `--` is just another ARGV entry -- ruby's own answer.
        assert_eq!(ok(&["t.rb", "--", "x"]).program_args, vec!["--", "x"]);
        // An -e compile (-o given) has no ARGV to give.
        assert!(err(&["-e", "1", "-o", "bin", "x"]).contains("unexpected argument"));
    }

    #[test]
    pub(crate) fn dash_i_accepts_all_three_spellings() {
        let a = ok(&["-I", "a", "-Ib", "-I=c", "t.rb"]);
        assert_eq!(a.load_roots, ["a", "b", "c"].map(PathBuf::from).to_vec());
    }

    #[test]
    pub(crate) fn dashdash_lets_a_dash_leading_filename_through() {
        let a = ok(&["--", "-weird.rb"]);
        assert!(matches!(a.source, Source::File(ref p) if p == &PathBuf::from("-weird.rb")));
    }

    #[test]
    pub(crate) fn rubyopt_is_restricted_and_the_command_line_wins() {
        let env = Env {
            rubyopt: Some("-W0 -Ia".to_string()),
            rubylib: Some("x:y".into()),
            ..Env::default()
        };
        let a = match parse_env(&["-Ib", "-W:deprecated", "t.rb"], &env).unwrap() {
            Parsed::Run(a) => *a,
            _ => panic!(),
        };
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
    pub(crate) fn informational_modes() {
        assert!(matches!(parse(&["--help"]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["-h"]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["--version"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(&["-v"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(&[]).unwrap(), Parsed::NoInput));
    }

    #[test]
    pub(crate) fn log_level_takes_a_bare_level_or_a_directive() {
        assert_eq!(
            ok(&["--log-level", "debug", "t.rb"]).log_level.as_deref(),
            Some("debug")
        );
        assert_eq!(
            ok(&["--log-level=info", "t.rb"]).log_level.as_deref(),
            Some("info")
        );
        assert_eq!(ok(&["t.rb"]).log_level, None);
        assert!(err(&["--log-level"]).contains("requires a value"));
        // A bare level widens to the whole compiler; a directive is passed
        // through, so one module can still be singled out.
        assert_eq!(widen_bare_level("debug"), "zeo=debug,zeo_rt=debug");
        assert_eq!(
            widen_bare_level("crate::analyze=trace"),
            "crate::analyze=trace"
        );
        assert_eq!(
            widen_bare_level("zeo=info,zeo_rt=warn"),
            "zeo=info,zeo_rt=warn"
        );
    }

    #[test]
    pub(crate) fn emit_clif_takes_an_attached_path_or_stdout() {
        assert!(matches!(
            ok(&["--emit-clif", "t.rb"]).emit_clif,
            Some(EmitTarget::Stdout)
        ));
        match ok(&["--emit-clif=out.clif", "t.rb"]).emit_clif {
            Some(EmitTarget::File(p)) => assert_eq!(p, PathBuf::from("out.clif")),
            other => panic!("expected a file target, got {:?}", other.is_some()),
        }
        // The retired Rust emitter's flags are gone, and say so.
        assert!(err(&["--emit-rust", "t.rb"]).contains("invalid option"));
        assert!(err(&["--pretty", "t.rb"]).contains("invalid option"));
        assert!(err(&["--dump=rust", "t.rb"]).contains("not a dump zeo knows"));
    }

    #[test]
    pub(crate) fn dump_is_the_ruby_spelling_of_the_inspect_modes() {
        assert!(matches!(
            ok(&["--dump=clif", "t.rb"]).emit_clif,
            Some(EmitTarget::Stdout)
        ));
        assert!(ok(&["--dump=syntax", "t.rb"]).check_syntax);
        assert!(ok(&["-c", "t.rb"]).check_syntax);
        assert!(!ok(&["t.rb"]).check_syntax);

        // Refused rather than warned about: running while printing nothing
        // is the silent drop this CLI exists to avoid.
        assert!(err(&["--dump=insns", "t.rb"]).contains("emits no bytecode"));
        assert!(err(&["--dump=parsetree", "t.rb"]).contains("carry no printer"));
        assert!(err(&["--dump", "t.rb"]).contains("needs a kind"));

        // An inspect mode has no artifact, so a flag describing one is an
        // error rather than something quietly dropped.
        for flag in [
            vec!["--dump=clif", "-o", "out", "t.rb"],
            vec!["--dump=clif", "--compile", "t.rb"],
            vec!["--dump=clif", "--backend", "jit", "t.rb"],
            vec!["--dump=clif", "-g", "t.rb"],
        ] {
            assert!(err(&flag).contains("would be ignored"), "{flag:?}");
        }
        for flag in [
            vec!["-c", "-o", "out", "t.rb"],
            vec!["-c", "--backend", "aot", "t.rb"],
        ] {
            assert!(err(&flag).contains("would be ignored"), "{flag:?}");
        }
        assert!(err(&["--dump=clif", "--dump=syntax", "t.rb"]).contains("ask for one"));

        // `units` is zeo's own kind, and behaves like the other inspect
        // modes: no artifact, no ARGV, and not combinable.
        assert!(matches!(
            ok(&["--dump=units", "t.rb"]).dump_front_end,
            Some(FrontEndDump::Units)
        ));
        assert!(matches!(
            ok(&["--dump=classes", "t.rb"]).dump_front_end,
            Some(FrontEndDump::Classes(None))
        ));
        // The filter rides ON the kind, so it cannot be confused with the
        // input file the way a spaced value would be.
        match ok(&["--dump=classes=Bundler::Source", "t.rb"]).dump_front_end {
            Some(FrontEndDump::Classes(Some(f))) => assert_eq!(f, "Bundler::Source"),
            _ => panic!("expected a filtered classes dump"),
        }
        assert!(ok(&["t.rb"]).dump_front_end.is_none());
        assert!(err(&["--dump=units", "-o", "out", "t.rb"]).contains("would be ignored"));
        assert!(err(&["--dump=units", "--dump=clif", "t.rb"]).contains("ask for one"));
        assert!(err(&["--dump=units", "t.rb", "arg"]).contains("unexpected argument"));

        // None of them takes program ARGV.
        assert!(err(&["-c", "t.rb", "arg"]).contains("unexpected argument"));
    }
}
