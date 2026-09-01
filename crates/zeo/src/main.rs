//! The `zeo` CLI: argument parsing plus calling into the `zeo` library
//! (`run_jit_with` to run, `compile_to_object_with` + the `cc` link for an
//! artifact) -- see `lib.rs` for the parse -> lower -> analyze -> clif
//! pipeline.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
// The subcommand drivers live in the library, where the parity probe reads
// the same text to run under CRuby.
use zeo::subcommand::driver as subcommand_driver;

/// What `run` can fail with: a compile error renders as a miette diagnostic
/// (annotated source excerpt, auto-degrading for pipes/NO_COLOR); everything
/// else (argument parsing, IO, the `cc` link step) keeps the
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
    /// `-I` roots, then RUBYOPT's `-I` roots, then RUBYLIB -- ruby's order.
    load_roots: Vec<PathBuf>,
    /// `-r <lib>`: libraries required before the program's first line, in the
    /// order given (repeatable).
    required_libraries: Vec<String>,
    /// `--gems <dir>`: vendored-gem directories (repeatable).
    package_dirs: Vec<PathBuf>,
    /// `--embed-sources <dir>`: directories whose `.rb` files travel INSIDE
    /// the program, for a `require` only the run time can resolve
    /// (repeatable).
    embed_sources: Vec<PathBuf>,
    /// `--strict-static-require`: a `require`/`load` target the compiler
    /// cannot resolve is an error HERE, not at run time.
    strict_static_require: bool,
    /// `--package <feature>` -- compile the positional file as a separately
    /// linked package for that feature spelling; `-o` names the artifact
    /// (default: `<feature basename>.zeopkg` in the current directory).
    pkg_feature: Option<String>,
    /// `--with-package <artifact>` (repeatable) -- merge that package
    /// (a `.zeopkg` bundle, or an object with `<object>.zman` beside it)
    /// into this program.
    with_packages: Vec<PathBuf>,
    /// `--root-gem <name>`: the distinguished root package -- it outranks
    /// every other provider for an ambiguous feature (Bundler-root
    /// semantics). The gem probe names its subject here.
    root_gem: Option<String>,
    /// `--report[=<path>]`: the `zeo-gems.json` disclosure record, opt-in.
    report: Report,
    /// The external gem store dirs (`--gem-path`/`GEM_PATH`) and the lockfile
    /// derived from `--bundle-gemfile`/`BUNDLE_GEMFILE`. Either both are
    /// populated or neither -- `parse_args_from` enforces the pairing.
    gem_paths: Vec<PathBuf>,
    lockfile: Option<PathBuf>,
    /// Whether a store FLAG was given (vs env-only): flags demand a strict
    /// "lockfile must exist" check, ambient env degrades quietly.
    store_from_flags: bool,
    /// `--compile`: write the default-named binary (the input path with its
    /// extension stripped) instead of running.
    ///
    /// Running is the DEFAULT: a bare `zeo foo.rb` compiles and executes,
    /// exactly like `ruby foo.rb` (a deliberate reversal of the original
    /// opt-in-run decision -- ruby's mental model won). An artifact is what
    /// needs asking for now: `-o <path>` or this flag.
    compile: bool,
    /// ARGV for an immediately-run program (`-e`, or a file that runs):
    /// positionals and everything after `--`, exactly ruby's
    /// `[--] [args...]` shape.
    program_args: Vec<String>,
    /// `--emit-clif[=<path>]` / `--dump=clif`: emit the Cranelift IR instead
    /// of building -- to the attached path or stdout when bare. Implies the
    /// aot pipeline.
    emit_clif: Option<EmitTarget>,
    /// `--dump=syntax` (and `-c`): parse, print `Syntax OK`, and stop.
    check_syntax: bool,
    /// `--dump=units` / `--dump=classes[=<filter>]`: run the front end,
    /// print the named report, and stop.
    dump_front_end: Option<FrontEndDump>,
    /// `--backend <aot|jit>`: which Cranelift mode builds the program
    /// (`ZEO_BACKEND` is the env spelling; the flag wins). `None` = the
    /// default for the mode, which `Backend::select` decides.
    backend: Option<zeo::backend::Backend>,
    /// `-g`: put DWARF line tables in the emitted object, so a native
    /// debugger or profiler renders a compiled frame as `file.rb:line`.
    /// `ZEO_DEBUGINFO=1` is the env spelling.
    debuginfo: bool,
    /// `--log-level <level|directive>`: the compiler's own `tracing` output.
    /// A bare level widens to every zeo crate; anything with a `=` or a `,`
    /// is passed to `EnvFilter` as written. Wins over `ZEO_LOG`/`RUST_LOG`,
    /// because a flag is more specific than an ambient variable.
    log_level: Option<String>,
}

/// A `--dump` kind the FRONT END answers: no emission, no artifact, no
/// backend -- see `zeo::dump`.
enum FrontEndDump {
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
enum EmitTarget {
    Stdout,
    File(PathBuf),
}

enum Source {
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
const IRB_DRIVER: &str = "require \"irb\"\nIRB.start\n";


/// Whether zeo was invoked from an interactive terminal, which is what makes
/// a bare `zeo` a shell rather than an error. Both ends are asked: a piped
/// stdin has a program to read, and a redirected stdout has nothing to draw a
/// prompt on.
fn interactive_terminal() -> bool {
    // SAFETY: `isatty` reads a descriptor number and touches nothing else.
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
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
    /// `zeo install`: precompile the project's locked gems into the store.
    Install(InstallCmd),
    /// `-h`/`--help` (exit 0).
    Help,
    /// `-v`/`--version` (exit 0).
    Version,
    /// Bare `zeo`: help, but exit 1 -- an invocation that compiled nothing
    /// must not look like success to a caller that expected an artifact.
    NoInput,
}

/// `zeo install [--bundle-gemfile <path>] [--gem-path <dir>]... [names...]`.
///
/// The verb precompiles the lockfile's gems into the store tier; it never
/// runs Bundler (that is `zeo bundle install`) and never touches the
/// network. Bare names narrow it to those gems.
#[derive(Debug, Default, PartialEq)]
struct InstallCmd {
    gemfile: Option<PathBuf>,
    gem_paths: Vec<PathBuf>,
    names: Vec<String>,
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
zeo -- compile Ruby to a native binary, or run it like ruby

usage: zeo [options] (<input.rb> [args...] | -e <code> [--] [args...])

modes:
  <input.rb>            compile the file and RUN it immediately, like ruby:
                        stdout/stderr and the exit status are forwarded, and
                        everything after the file name becomes the program's
                        ARGV. Option parsing STOPS there, ruby's own rule, so
                        `zeo test.rb --seed 42 -v` passes all three on;
                        zeo's own options go BEFORE the file name
  build <input.rb>      compile the file to a native binary without running
                        it: `-o <path>` names the binary (default: the input
                        path with its extension stripped). A build has no
                        program ARGV, so its options may also FOLLOW the
                        file: `zeo build app.rb -o dist/app`
  -o <path> <input.rb>  compile the file to a native binary at <path>
                        instead of running it
  --compile <input.rb>  compile to the default output path (the input path
                        with its extension stripped) without running
  -e <code>             compile and run an inline program immediately
                        (repeatable; snippets are joined with newlines);
                        trailing [args...] become the program's ARGV;
                        with -o, write the binary instead of running it
  (no arguments)        open an irb shell, when there is a terminal to talk
                        to; piping or redirecting zeo is unaffected
  --irb                 open the shell whether or not there is a terminal --
                        the same thing by name, for a wrapper or a pty

subcommands:
  gem <args...>         run rubygems -- the real one, compiled from the
                        vendored library, so `zeo gem install rack` needs no
                        ruby on the machine
  bundle <args...>      run bundler, likewise -- `zeo bundle install` resolves
                        and installs a Gemfile with no ruby on the machine.
                        `bundle exec` and `bundler/setup` do not work yet:
                        both ask rubygems for an INSTALLED bundler gem,
                        and zeo carries bundler as a library. See
                        docs/COMPATIBILITY.md.
  install [names...]    precompile the project's locked gems into the gem
                        store, so later compiles link them instead of
                        recompiling them (bare names narrow it). Reads
                        Gemfile.lock and the installed store; never runs
                        Bundler and never touches the network -- run
                        `zeo bundle install` first. --bundle-gemfile and
                        --gem-path (or their env spellings) relocate it; a
                        gem the package tier cannot carry is reported and
                        keeps compiling from source
                        A script really named `build`, `gem`, `bundle` or
                        `install` still runs as `zeo ./build`; a verb never
                        depends on what is in the current directory.

options:
  -o <output>           where to write the compiled binary
  --compile             write the default-named binary instead of running
  --emit-clif[=<path>]  emit the Cranelift IR (the aot backend's own
                        lowering) instead of building; bare prints to stdout
  --dump=<kind>         inspect instead of building. `clif` is --emit-clif
                        to stdout; `syntax` parses and prints `Syntax OK`
                        (`-c` is the short spelling); `units` prints the
                        compiled-in load path -- every file that became a
                        feature unit, with the spellings a require can use
                        for it; `classes[=<filter>]` says which class bodies
                        ever run; `methods[=<rows>]` counts what inheritance
                        costs, since one `def` is emitted once per class that
                        carries it. `insns` and `parsetree` are REFUSED rather
                        than warned about: zeo emits no bytecode, and the
                        prism tree has no printer on the Rust side
  -c                    --dump=syntax, ruby's short spelling
  --backend <aot|jit>   which mode the Cranelift backend runs in: the
                        in-process JIT (the default in run mode) or the AOT
                        object-file path (the default with -o)
                        (ZEO_BACKEND is the env spelling; the flag wins)
  -g                    put DWARF line tables in the compiled program, so
                        lldb, perf and Instruments name a Ruby frame by its
                        file and line (ZEO_DEBUGINFO=1 is the env spelling)
  --log-level <level>   narrate the compiler's own work on stderr: a bare
                        level (error, warn, info, debug, trace) covers the
                        whole compiler, and a `tracing` directive
                        (`zeo::analyze=debug`) narrows it. Beats ZEO_LOG and
                        RUST_LOG, which take the directive form only
  -I <dir>              add a `require` search root, like ruby's -I
                        (repeatable; `-I<dir>` and `-I=<dir>` also accepted)
  -r <library>          require a library before the program's first line,
                        like ruby's -r (repeatable, in the order given;
                        `-r<library>` also accepted)
  --gems <dir>          add a directory of vendored gems: every subdirectory
                        with a `.gemspec` is discovered as a gem (repeatable)
  --embed-sources <dir> carry this directory's `.rb` files INSIDE the program,
                        so a require only the run time can resolve -- a
                        computed feature name -- finds them without a
                        filesystem (repeatable; off by default, since a
                        hermetic binary is the point)
  --strict-static-require
                        refuse at COMPILE time a `require`/`load` whose
                        target this compile cannot resolve, instead of
                        leaving it to the run-time loader
  --package <feature>   compile <input.rb> as a precompiled PACKAGE for that
                        require spelling, instead of as a program: the
                        artifact is a `.zeopkg` a later compile links with
                        --with-package (default output: `<feature>.zeopkg`;
                        -o renames it)
  --with-package <artifact>
                        link a precompiled package into this program
                        (repeatable). An artifact is accepted only when its
                        compiler, target and interface hashes match exactly;
                        anything else is refused by name
  --root-gem <name>     treat the named gem as the root package: it outranks
                        every other provider when a feature is found in
                        multiple gems (Bundler-root semantics)
  --gem-path <dir>      an installed RubyGems store (`gem env gemdir`) to
                        resolve locked gems against (repeatable; defaults to
                        GEM_PATH); needs a Gemfile via --bundle-gemfile
  --bundle-gemfile <path>
                        the Gemfile whose lockfile (`<path>.lock`) selects
                        the store gem versions (defaults to BUNDLE_GEMFILE);
                        needs a store via --gem-path
  --report[=<path>]     write the `zeo-gems.json` disclosure record
                        (default path: next to the output artifact)
  --enable=<features>   turn on what the program starts with, before its own
  --disable=<features>  first line -- ruby's own names, comma-separated, plus
                        `all`. `--enable-gems` is accepted too, and so is
                        either spelling of a name. The features, and where
                        this build leaves each one, are listed below
  -w, -W[0-2]           accepted, ruby's shapes; zeo warns from neither
  -W:[no-]<category>    accepted for ruby's categories (deprecated,
                        experimental, performance, strict_unused_block)
  -v, --version         print the version and exit
  -h, --help            show this message

Long options also accept the attached `--flag=<value>` spelling.

require search order (first gem with a given name wins):
  1. -I roots in the order given, then RUBYLIB entries
  2. --gems dirs in the order given
  3. the input file's sibling gems/ directory
  4. zeo's own bundled libraries, then the external gem store

Zeo carries its own stdlib -- uri, csv, json, rubygems, bundler and ~70 more
-- and step 4 reaches THAT copy, not the machine's. Nothing a `gem install`
put in a store is visible to a compile until --gem-path and --bundle-gemfile
name it, so an installed zeo compiles the same program the same way on every
machine. `--report` writes down which one answered each require.

environment:
  RUBYOPT               extra leading options (only -I, -w and -W allowed)
  RUBYLIB               extra `require` search roots, after every -I
  GEM_PATH              gem store dirs for --gem-path; activates only when a
                        Gemfile is also known (an ambient store alone never
                        changes a compile)
  BUNDLE_GEMFILE        the Gemfile for --bundle-gemfile
  ZEO_BACKEND           `jit` or `aot` -- override the default backend (jit
                        for immediate runs, aot for -o/--compile)
  ZEO_LOG / RUST_LOG    a `tracing` EnvFilter directive for zeo's internal
                        logs, e.g. `zeo=debug` or
                        `zeo::analyze=debug,zeo::lower=trace`. --log-level is
                        the flag spelling, and it wins. `zeo_rt=debug` asks the
                        RUNTIME instead -- what the program defined, aliased,
                        required and loaded, and where each write landed. Pair
                        it with ZEO_CACHE=0: a cached program runs the runtime
                        it was built with, which may predate the logging
  ZEO_MEMORY_LIMIT      bytes of resident memory this compile may use before it
                        gives up (default: half the machine's RAM, capped at
                        8 GiB; 0 compiles unbounded, which can exhaust the
                        machine). A breach exits 12 and names the phase.
";

fn parse_args() -> Result<Parsed, String> {
    parse_args_from(std::env::args().skip(1).collect(), &Env::from_process())
}

fn parse_args_from(argv: Vec<String>, env: &Env) -> Result<Parsed, String> {
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
    let mut check_syntax = false;
    let mut dump_front_end: Option<FrontEndDump> = None;
    // The env spelling is read once here so the flag and the variable can
    // never disagree downstream.
    let mut debuginfo = std::env::var_os("ZEO_DEBUGINFO").is_some_and(|v| v != "0");
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
    let mut backend: Option<zeo::backend::Backend> = None;
    let mut program_args: Vec<String> = Vec::new();
    let mut required_libraries: Vec<String> = Vec::new();
    let mut features = zeo::ruby_features::RubyFeatures::default();

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
                    .or_else(|| name.strip_prefix("disable-").map(|f| (f.to_string(), false))),
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
                "backend" => backend = Some(zeo::backend::Backend::parse(&value("--backend")?)?),
                "gems" => package_dirs.push(PathBuf::from(value("--gems")?)),
                "package" => {
                    pkg_feature = Some(value("--package")?);
                }
                "with-package" => {
                    with_packages.push(PathBuf::from(value("--with-package")?));
                }
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
                // ruby's own spelling for the same family. Where CRuby WARNS
                // for a kind it cannot serve and keeps running, zeo errors:
                // running while printing nothing is the silent drop the
                // project forbids, and the CLI already refuses flags on
                // purpose (`-S`, `--nowarn`).
                "dump" => match inline.as_deref().map(|k| k.split_once('=').unwrap_or((k, ""))) {
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
    for (flag, set) in [
        ("-o", output.is_some()),
        ("--compile", compile),
        ("--backend", backend.is_some()),
        ("-g", debuginfo),
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
    let runs_now =
        output.is_none() && !compile && emit_clif.is_none() && !check_syntax && dump_front_end.is_none();
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
        lockfile: gemfile.map(zeo::project::derive_lockfile),
        program_args,
        emit_clif,
        check_syntax,
        dump_front_end,
        debuginfo,
        backend,
        log_level,
    })))
}

/// The install verb's own flags: a store, a Gemfile, gem names. `-h` gets
/// the main help; anything else is refused by name.
fn parse_install(argv: &[String]) -> Result<InstallCmd, String> {
    let mut cmd = InstallCmd::default();
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
            "--bundle-gemfile" => cmd.gemfile = Some(PathBuf::from(value("--bundle-gemfile")?)),
            "--gem-path" => cmd.gem_paths.push(PathBuf::from(value("--gem-path")?)),
            _ if name.starts_with('-') => {
                return Err(format!(
                    "invalid option for zeo install: {name} (it takes --bundle-gemfile, \
                     --gem-path, and gem names)"
                ));
            }
            _ => cmd.names.push(arg.clone()),
        }
    }
    Ok(cmd)
}

/// Every `-W:[no-]<category>` ruby 4.0.6 accepts. zeo emits none of these
/// categories, so toggling one is a no-op -- but an unknown name is still
/// reported, exactly as ruby reports it, so a typo is not silently ignored.
const WARNING_CATEGORIES: &[&str] = &[
    "deprecated",
    "experimental",
    "performance",
    "strict_unused_block",
];

/// The `-w`/`-W` family, ruby's shapes. Every form is accepted and none
/// changes what zeo prints: the compiler's own diagnostics are errors, and
/// the warning categories above belong to a runtime zeo does not warn from.
/// An unknown category warns and continues, as ruby's own driver does.
fn warn_flag(arg: &str) -> Result<(), String> {
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
/// or used as a library. They come from the resolved home (see `zeo::home`);
/// `zeo::bundled` decides the set.
fn default_package_dirs(input: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = input.and_then(|p| p.parent()) {
        dirs.push(parent.join("gems"));
    }
    dirs
}

/// The help message, plus the `--enable`/`--disable` table rendered from the
/// dials themselves -- so what it says a build starts with is what that build
/// actually starts with.
fn print_help() {
    print!("{HELP}");
    println!("\nfeatures (--enable=<name> / --disable=<name>):");
    for line in zeo::ruby_features::help_lines() {
        println!("  {line}");
    }
}

fn run() -> Result<(), MainError> {
    let mut args = match parse_args()? {
        Parsed::Run(args) => args,
        Parsed::Install(cmd) => return run_install(cmd),
        Parsed::Help => {
            print_help();
            return Ok(());
        }
        Parsed::Version => {
            println!("zeo {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Parsed::NoInput => {
            print_help();
            std::process::exit(1);
        }
    };
    init_tracing(args.log_level.as_deref());
    // Pre-flight: a broken install (payload missing next to the executable)
    // reports here as an ordinary error instead of panicking mid-compile.
    zeo::home::ensure_resolved()?;
    let (source, input_path) = match &args.source {
        Source::File(path) => (zeo::parse::read_source(path)?, Some(path.clone())),
        Source::Eval(code) => (code.clone(), None),
        Source::Irb => (IRB_DRIVER.to_string(), None),
    };
    // A package build compiles its entry as a FEATURE
    // UNIT, not as `<main>` -- the main source is empty and the entry rides
    // `CompileOptions::package_build` into the loader.
    let source = if args.pkg_feature.is_some() {
        String::new()
    } else {
        source
    };
    // Armed before the compile, not inside it: the ceiling covers the whole
    // compile, emission, and link. The
    // library entry point deliberately does NOT arm one -- an in-process
    // caller (the test harness) owns its own process and must not have it
    // exited out from under it.
    zeo::memguard::arm(&match &args.source {
        Source::File(path) => path.display().to_string(),
        Source::Eval(_) => "-e".to_string(),
        Source::Irb => "irb".to_string(),
    });

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
    // The package options. A package build takes the
    // positional file as its ENTRY and `-o` as its object; the manifest
    // lands beside the object as `<output>.zman`. A host names package
    // artifacts with `--with-package`; their manifests are read here
    // so the compile is a function of their TEXT (and the object digest
    // keeps the program cache honest about a body-only rebuild).
    let package_build = match &args.pkg_feature {
        Some(feature) => {
            let Source::File(entry) = &args.source else {
                return Err("--package needs a gem entry file".to_string().into());
            };
            let Some(out) = &args.output else {
                return Err("--package needs -o <artifact path>".to_string().into());
            };
            Some(zeo::package::PackageBuild {
                entry: entry.clone(),
                feature: feature.clone(),
                manifest_out: out.with_extension("zman"),
                root: entry
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .canonicalize()
                    .map_err(|e| format!("resolving {}: {e}", entry.display()))?,
            })
        }
        None => None,
    };
    // The store tier: a locked project's precompiled artifacts link instead
    // of recompiling. Only a compile with an ACTIVE store consults it (the
    // same pairing rule the store roots follow), a package build never does
    // (a package compiles alone), and an artifact the merge later refuses
    // drops back to a source compile with a warning.
    if package_build.is_none()
        && !args.gem_paths.is_empty()
        && let Some(lock) = &args.lockfile
    {
        for artifact in zeo::project::store_linkable(lock, &args.gem_paths) {
            if !args.with_packages.contains(&artifact) {
                args.with_packages.push(artifact);
            }
        }
    }
    let use_packages: Vec<zeo::package::UsePackage> = args
        .with_packages
        .iter()
        .map(|obj| {
            // Two artifact spellings: a bare object with the manifest
            // beside it, or the single-file `.zeopkg` bundle. A bundle's
            // object lands in the content-addressed pool, where the link
            // line can name it.
            if obj.extension().is_some_and(|e| e == "zeopkg") {
                let (manifest_text, bytes) = zeo::package::read_zeopkg(obj)?;
                let object_digest = zeo::package::fnv64(&bytes);
                let object = zeo::progcache::pkg_object_file(object_digest, &bytes)
                    .map_err(|e| format!("unpacking {}: {e}", obj.display()))?;
                return Ok(zeo::package::UsePackage {
                    manifest_path: obj.clone(),
                    manifest_text,
                    object,
                    object_digest,
                });
            }
            let manifest_path = obj.with_extension("zman");
            let manifest_text = std::fs::read_to_string(&manifest_path)
                .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
            let bytes = std::fs::read(obj).map_err(|e| format!("reading {}: {e}", obj.display()))?;
            Ok(zeo::package::UsePackage {
                manifest_path,
                manifest_text,
                object: obj.clone(),
                object_digest: zeo::package::fnv64(&bytes),
            })
        })
        .collect::<Result<_, String>>()?;
    let opts = zeo::CompileOptions {
        input_path: input_path.clone(),
        // A package build's MAIN source is synthetic (empty -- the entry
        // rides in as a feature unit), so it carries a synthetic name: the
        // entry path would otherwise register with empty text, and the
        // package cache's manifest could never verify that row.
        file_name: package_build
            .as_ref()
            .map(|pb| std::path::PathBuf::from(format!("<package {}>", pb.feature))),
        line_offset: 0,
        mode: zeo::CompileMode::Program,
        load_roots: args.load_roots.clone(),
        package_dirs,
        gem_report,
        gem_paths: args.gem_paths.clone(),
        lockfile: args.lockfile.clone(),
        root_gem: args.root_gem.clone().map(zeo::Gem::named),
        embed_sources: args.embed_sources.clone(),
        strict_static_require: args.strict_static_require,
        required_libraries: args.required_libraries.clone(),
        package_build,
        use_packages,
    };
    // Parse only, then say so -- ruby's `Syntax OK`, byte for byte. A syntax
    // error reports itself the way every other compile error does, so the
    // exit status separates the two.
    if args.check_syntax {
        zeo::check_syntax(&source)?;
        println!("Syntax OK");
        return Ok(());
    }
    // The two front-end reports, before anything is emitted: which files
    // became units and under which spellings, and which classes wait for a
    // unit to reveal them. Both are analyze-level answers, so both cost the
    // front end and nothing more -- seconds against the minute a full
    // compile of a 500-unit gem takes.
    if let Some(kind) = &args.dump_front_end {
        let analyzed = zeo::analyze_program(&source, &opts)?;
        print!(
            "{}",
            match kind {
                FrontEndDump::Units => zeo::dump::units(&analyzed),
                FrontEndDump::Classes(filter) => zeo::dump::classes(&analyzed, filter.as_deref()),
                FrontEndDump::Methods(top) => zeo::dump::methods(&analyzed, *top),
            }
        );
        return Ok(());
    }
    if let Some(target) = &args.emit_clif {
        let text = zeo::compile_to_clif_text(&source, &opts)?;
        match target {
            EmitTarget::Stdout => print!("{text}"),
            EmitTarget::File(path) => std::fs::write(path, &text)
                .map_err(|e| format!("writing {}: {e}", path.display()))?,
        }
        return Ok(());
    }

    // The backend decides whether this compile runs in place or produces an
    // object file, so it is selected before compiling.
    let wants_artifact = args.compile || args.output.is_some();
    let backend = zeo::backend::Backend::select(args.backend, wants_artifact)?;
    // The JIT is run-in-place by definition: compile into this process and
    // exit with the program's status. An artifact request needs a backend
    // that produces one.
    // A package build writes its artifact and stops -- there
    // is nothing to run or link. It goes through the machine-wide package
    // cache first.
    if opts.package_build.is_some() {
        let out = args.output.as_ref().expect("--package checked -o above");
        return build_package(&source, &opts, out);
    }
    if backend == zeo::backend::Backend::Jit {
        // Silently honouring nothing is the one answer that would be
        // wrong: DWARF describes an artifact, and the in-process JIT
        // leaves none behind.
        if args.debuginfo {
            return Err(
                "-g describes a compiled artifact and the in-process JIT produces none (use -o/--compile)"
                    .to_string()
                    .into(),
            );
        }
        if wants_artifact {
            return Err(
                "--backend jit runs in place and produces no artifact (use --backend aot for -o/--compile)"
                    .to_string()
                    .into(),
            );
        }
        // The cache runs a program AHEAD of time and execs it, so an
        // unchanged program never compiles twice. It is tried first, and
        // falling through to the JIT below is the answer whenever it cannot
        // help -- see `run_from_cache`.
        let program_name = match &args.source {
            Source::File(path) => path.display().to_string(),
            Source::Eval(_) => "-e".to_string(),
            Source::Irb => "irb".to_string(),
        };
        if args.backend.is_none() && zeo::progcache::enabled() {
            run_from_cache(&source, &opts, &program_name, &args.program_args);
        }
        if !opts.use_packages.is_empty() {
            return Err(
                "--with-package needs the AOT path: the in-process JIT cannot \
                 link a package object (use -o/--compile, or leave the program cache on)"
                    .to_string()
                    .into(),
            );
        }
        match zeo::run_jit_with(&source, &opts, &program_name, &args.program_args)? {}
    }
    let compiled = match backend {
        zeo::backend::Backend::Aot => {
            compile_dropping_refused_packages(&source, &opts, args.debuginfo)?
        }
        zeo::backend::Backend::Jit => unreachable!("the jit branch above never falls through"),
    };
    zeo::memguard::set_phase(zeo::memguard::Phase::Build);
    let program = zeo::backend::CompiledProgram::Aot(&compiled);

    // The default mode -- a bare file or `-e`, no artifact asked for: build a
    // throwaway program, run it, and exit with ITS status (stdout/stderr
    // stream straight through), like ruby. With `-o` or `--compile`, produce
    // an artifact instead. The mode bodies live in `backend`; this is just the
    // mode decision.
    if !args.compile && args.output.is_none() {
        match zeo::backend::run_program(&program, &args.program_args)? {}
    }

    let output = args.output.unwrap_or_else(|| {
        let mut p = match &args.source {
            Source::File(path) => path.clone(),
            Source::Eval(_) | Source::Irb => {
                unreachable!("a pathless source always has -o")
            }
        };
        p.set_extension("");
        p
    });
    Ok(zeo::backend::build_artifact(&program, &output)?)
}

/// Compile a package, through the machine-wide package
/// cache. `-o pkg.zeopkg` writes the single-file artifact; any other `-o`
/// writes the raw object with the manifest beside it. A cache write that
/// fails (a read-only cache directory) degrades to "compiled, not cached"
/// with a log line, never to a failed build.
/// `zeo install`: precompile the project's locked gems into the store tier.
///
/// Resolution is Bundler's, already written down -- this verb reads the
/// lockfile and the installed store, compiles each gem it can to a
/// `.zeopkg` (through the machine cache), and places the artifact at its
/// store home. It never runs Bundler and never touches the network; a
/// missing store says to run `zeo bundle install` first. A gem the
/// package tier cannot carry is a `skip` or `declined` row, never an
/// error: its require simply keeps compiling from source.
fn run_install(cmd: InstallCmd) -> Result<(), MainError> {
    init_tracing(None);
    zeo::home::ensure_resolved()?;
    let env = Env::from_process();
    let gemfile = cmd
        .gemfile
        .or_else(|| env.bundle_gemfile.as_ref().map(PathBuf::from));
    let project = zeo::project::locate(
        gemfile,
        cmd.gem_paths,
        std::env::var_os("GEM_PATH").as_deref(),
    )?;
    let rows = zeo::project::survey(&project)?;
    for name in &cmd.names {
        if !rows.iter().any(|r| &r.name == name) {
            return Err(format!(
                "`{name}` is not a gem in {}",
                project.lockfile.display()
            )
            .into());
        }
    }
    zeo::memguard::arm("install");
    let (mut installed, mut cached_only, mut skipped, mut declined) = (0u32, 0u32, 0u32, 0u32);
    for row in &rows {
        if !cmd.names.is_empty() && !cmd.names.contains(&row.name) {
            continue;
        }
        let label = format!("{} {}", row.name, row.version);
        if let Some(reason) = &row.skip {
            println!("     skip {label} ({reason})");
            skipped += 1;
            continue;
        }
        let (entry, home) = (
            row.entry.as_ref().expect("no skip means an entry"),
            row.home.as_ref().expect("an entry has a home"),
        );
        match install_one(&row.feature, entry, home) {
            Ok(true) => {
                println!("  install {label}");
                installed += 1;
            }
            Ok(false) => {
                println!("    cache {label} (store not writable; kept in the machine cache)");
                cached_only += 1;
            }
            Err(reason) => {
                println!("  decline {label} ({reason})");
                declined += 1;
            }
        }
    }
    println!(
        "zeo install: {installed} installed, {cached_only} cached, {skipped} skipped, \
         {declined} declined"
    );
    Ok(())
}

/// Compile one gem to a `.zeopkg` and place it at its store `home`.
/// `Ok(false)` = built, but the store is read-only (the machine cache still
/// holds it). `Err` carries the refusal's first line.
fn install_one(
    feature: &str,
    entry: &std::path::Path,
    home: &std::path::Path,
) -> Result<bool, String> {
    let staging = std::env::temp_dir().join(format!(
        "zeo-install-{}-{}.zeopkg",
        std::process::id(),
        zeo::package::fnv64(home.as_os_str().as_encoded_bytes())
    ));
    let root = entry
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .canonicalize()
        .map_err(|e| format!("resolving {}: {e}", entry.display()))?;
    let opts = zeo::CompileOptions {
        input_path: Some(entry.to_path_buf()),
        file_name: Some(PathBuf::from(format!("<package {feature}>"))),
        mode: zeo::CompileMode::Program,
        package_build: Some(zeo::package::PackageBuild {
            entry: entry.to_path_buf(),
            feature: feature.to_string(),
            manifest_out: staging.with_extension("zman"),
            root,
        }),
        ..Default::default()
    };
    // The package build's MAIN source is synthetic and empty; the entry
    // rides in as a feature unit (see the host compile above).
    let first_line = |e: MainError| {
        let text = match e {
            MainError::Plain(m) => m,
            MainError::Compile(e) => e.to_string(),
        };
        text.lines().next().unwrap_or_default().to_string()
    };
    build_package("", &opts, &staging).map_err(first_line)?;
    let placed = zeo::package::store_install(home, &staging)
        .map_err(|e| format!("installing {}: {e}", home.display()))?;
    let _ = std::fs::remove_file(&staging);
    Ok(placed)
}

fn build_package(
    source: &str,
    opts: &zeo::CompileOptions,
    out: &std::path::Path,
) -> Result<(), MainError> {
    let bundled = out.extension().is_some_and(|e| e == "zeopkg");
    let pb = opts.package_build.as_ref().expect("a package build");
    let manifest_out = pb.manifest_out.clone();
    let key = zeo::progcache::pkg_key(source, opts);
    if zeo::progcache::enabled()
        && let Some(hit) = zeo::progcache::pkg_lookup(&key)
    {
        let (manifest_json, object) = zeo::package::read_zeopkg(&hit)?;
        return place_package(out, bundled, &manifest_json, &object, &manifest_out);
    }
    let compiled = zeo::compile_to_object_with(source, opts, false)?;
    let manifest_json = std::fs::read_to_string(&manifest_out)
        .map_err(|e| format!("reading {}: {e}", manifest_out.display()))?;
    place_package(out, bundled, &manifest_json, &compiled.object, &manifest_out)?;
    if zeo::progcache::enabled() {
        let cached: std::io::Result<()> = (|| {
            let slot = zeo::progcache::pkg_reserve(&key)?;
            zeo::package::write_zeopkg(&slot, &manifest_json, &compiled.object)?;
            zeo::progcache::pkg_commit(&key, &compiled.inputs)
        })();
        if let Err(e) = cached {
            tracing::warn!("could not record the package cache entry: {e}");
        }
    }
    Ok(())
}

/// Land a package's two halves at `-o`.
fn place_package(
    out: &std::path::Path,
    bundled: bool,
    manifest_json: &str,
    object: &[u8],
    manifest_out: &std::path::Path,
) -> Result<(), MainError> {
    if bundled {
        zeo::package::write_zeopkg(out, manifest_json, object)
            .map_err(|e| format!("writing {}: {e}", out.display()))?;
        // The bundle carries the manifest inside; a compile may have left
        // the loose copy beside the output.
        let _ = std::fs::remove_file(manifest_out);
    } else {
        std::fs::write(out, object).map_err(|e| format!("writing {}: {e}", out.display()))?;
        std::fs::write(manifest_out, manifest_json)
            .map_err(|e| format!("writing {}: {e}", manifest_out.display()))?;
    }
    Ok(())
}

/// The library's package-fallback compile, with each drop reported to
/// stderr as it happens -- so the warning still lands when a later error
/// (a feature whose source is nowhere) ends the compile.
fn compile_dropping_refused_packages(
    source: &str,
    opts: &zeo::CompileOptions,
    debuginfo: bool,
) -> Result<zeo::ObjectOutput, zeo::CompileError> {
    zeo::compile_to_object_with_package_fallback(source, opts, debuginfo, |d| {
        eprintln!(
            "zeo: warning: dropping the precompiled artifact for '{}' and \
             compiling it from source: {}",
            d.feature, d.reason
        );
    })
}

/// Run this program from the compiled-program cache, and never return.
///
/// RETURNS when the cache cannot answer, and every such path is a fall-through
/// to the in-process JIT rather than an error. That is the whole safety
/// argument for making this the default: the cache is an accelerator, and a
/// program it cannot build ahead of time runs exactly the way it always did.
/// Three shapes reach the JIT --
///
/// * `--report` writes a file the compile produces, which a cache HIT would
///   silently skip;
/// * the object compile refuses (a program that needs the compiler in its own
///   process is the JIT-only tier, `tests/jit/`), or the link does;
/// * writing into the cache directory fails.
///
/// A compile ERROR reaches the JIT too, which reports the same error one front
/// end later. A wrong program is slow to fail here, which is the right way
/// round.
fn run_from_cache(
    source: &str,
    opts: &zeo::CompileOptions,
    program_name: &str,
    program_args: &[String],
) {
    if opts.gem_report.is_some() {
        return;
    }
    let key = zeo::progcache::key(source, opts);
    if let Some(bin) = zeo::progcache::lookup(&key) {
        exec(&bin, program_name, program_args);
        return;
    }
    let Ok(compiled) = compile_dropping_refused_packages(source, opts, false) else {
        return;
    };
    let Ok(bin) = zeo::progcache::reserve(&key) else {
        return;
    };
    let program = zeo::backend::CompiledProgram::Aot(&compiled);
    if zeo::backend::build_artifact(&program, &bin).is_err() {
        return;
    }
    // The manifest is what makes the entry a HIT next time. Without it the
    // binary is there and unread, which costs a rebuild and nothing else.
    if let Err(e) = zeo::progcache::commit(&key, &compiled.inputs) {
        tracing::warn!("could not record the program cache manifest: {e}");
    }
    exec(&bin, program_name, program_args);
}

/// Replace this process with `bin`. `exec` rather than spawn-and-wait, so the
/// program keeps zeo's pid, its stdio, and its signal disposition -- the same
/// single process an in-process JIT run is.
///
/// `arg0` is the PROGRAM's name, not the cached binary's. `$0` is argv[0]
/// (`globals::seed_default_globals`), so without this a program would report
/// a path inside the cache: mkmf derives an extension's `srcdir` from `$0`,
/// and every generated Makefile came out pointing at the cache directory.
///
/// Returns only when the exec FAILED, which leaves the caller free to fall
/// back; the error is reported, because a cached binary that will not run is
/// worth knowing about even though the program still runs.
fn exec(bin: &std::path::Path, program_name: &str, program_args: &[String]) {
    use std::os::unix::process::CommandExt as _;
    let e = std::process::Command::new(bin)
        .arg0(program_name)
        .args(program_args)
        .exec();
    tracing::warn!("could not run the cached program {}: {e}", bin.display());
}

/// Install a `tracing` subscriber (stderr) for the compiler pipeline:
/// `--log-level` first, then `ZEO_LOG`, then `RUST_LOG` -- a flag is more
/// specific than an ambient variable, so it wins. All three take a full
/// `EnvFilter` directive (`zeo=debug`, `zeo::analyze=trace`); `--log-level`
/// additionally accepts a bare level, which widens to the whole compiler.
/// With none of them set, no subscriber is installed, so every
/// `trace!`/`debug!`/`instrument` in the pipeline compiles to a cheap disabled
/// check -- a normal compile stays silent and never interleaves with the
/// miette diagnostics.
fn init_tracing(flag: Option<&str>) {
    let Some(directive) = flag
        .map(widen_bare_level)
        .or_else(|| std::env::var("ZEO_LOG").ok().filter(|s| !s.is_empty()))
        .or_else(|| std::env::var("RUST_LOG").ok().filter(|s| !s.is_empty()))
    else {
        return;
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(directive))
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

/// `--log-level debug` means the whole compiler at debug, which is what the
/// six code comments naming this flag describe. A directive -- anything
/// carrying a `=` or a `,` -- is passed to `EnvFilter` untouched, so
/// `--log-level zeo::analyze=trace` still narrows to one module.
fn widen_bare_level(v: &str) -> String {
    match v.contains(['=', ',']) {
        true => v.to_string(),
        false => format!("zeo={v},zeo_rt={v}"),
    }
}

fn main() -> ExitCode {
    // A diagnostic read by a machine must not be reflowed. miette hard-wraps at
    // the terminal width and breaks after `/` and `-`, so an absolute path in a
    // message arrives split across two lines -- and whoever rejoins them cannot
    // tell the inserted space from a real one. That silently defeated a
    // harness's path scrubber and recorded 12 rows carrying this machine's
    // home directory. A pipe gets the message on one line; a terminal keeps the
    // wrapping, and colour still degrades on its own either way.
    if !std::io::stderr().is_terminal() {
        let _ = miette::set_hook(Box::new(|_| {
            Box::new(miette::MietteHandlerOpts::new().wrap_lines(false).build())
        }));
    }
    // The compiler IS the runtime's evaluator. Every `eval` is a run-time
    // compile; there is no interpreter to fall back to.
    zeo::eval::install();
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

    /// `zeo build` is `--compile` as a verb, and because a build has no
    /// program ARGV its options may follow the file.
    #[test]
    fn the_build_verb_compiles_without_running() {
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
    fn package_flags_parse_with_a_default_artifact_name() {
        let a = ok(&["build", "--package", "rack", "entry.rb"]);
        assert_eq!(a.pkg_feature.as_deref(), Some("rack"));
        assert_eq!(a.output.as_deref(), Some(std::path::Path::new("rack.zeopkg")));

        // A nested feature names the artifact by its basename.
        let a = ok(&["--package", "rack/utils", "-o", "x.zeopkg", "entry.rb"]);
        assert_eq!(a.output.as_deref(), Some(std::path::Path::new("x.zeopkg")));
        let a = ok(&["--package", "rack/utils", "entry.rb"]);
        assert_eq!(a.output.as_deref(), Some(std::path::Path::new("utils.zeopkg")));

        let a = ok(&["--with-package", "a.zeopkg", "--with-package", "b.zeopkg", "app.rb"]);
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
    fn a_subcommand_runs_the_vendored_library_and_keeps_its_own_flags() {
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

        // The verb is only a verb in FIRST position. A file really called
        // `gem` is reachable, and a file whose name merely contains it is
        // untouched.
        assert!(matches!(ok(&["./gem"]).source, Source::File(_)));
        assert!(matches!(ok(&["gemfile.rb"]).source, Source::File(_)));
        assert!(matches!(ok(&["-e", "1", "gem"]).source, Source::Eval(_)));
        assert_eq!(ok(&["-e", "1", "gem"]).program_args, ["gem"]);
    }

    #[test]
    fn a_bare_file_runs_and_an_artifact_needs_asking_for() {
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
    fn a_run_file_takes_argv_but_a_compiled_one_does_not() {
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
    fn gems_flag_collects_in_order_both_spellings() {
        let a = ok(&["--gems", "a", "--gems=b", "t.rb"]);
        assert_eq!(a.package_dirs, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn warning_flags_are_accepted_no_ops() {
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
    fn unknown_flags_are_rejected_not_swallowed() {
        assert!(err(&["--bogus", "t.rb"]).contains("invalid option"));
        assert!(err(&["-q", "t.rb"]).contains("invalid option"));
    }

    #[test]
    fn dead_flag_spellings_get_the_generic_rejection() {
        // The pointed migration errors served their year; old spellings now
        // fail like any other unknown option.
        for old in ["--packages", "--nowarn", "--lockfile", "--no-report"] {
            assert!(err(&[old, "t.rb"]).contains("invalid option"), "{old}");
        }
        assert!(err(&["-S", "t.rb"]).contains("invalid option"));
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
    fn eval_mode_collects_argv_like_ruby() {
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
    fn dash_i_accepts_all_three_spellings() {
        let a = ok(&["-I", "a", "-Ib", "-I=c", "t.rb"]);
        assert_eq!(a.load_roots, ["a", "b", "c"].map(PathBuf::from).to_vec());
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
    fn informational_modes() {
        assert!(matches!(parse(&["--help"]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["-h"]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["--version"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(&["-v"]).unwrap(), Parsed::Version));
        assert!(matches!(parse(&[]).unwrap(), Parsed::NoInput));
    }

    #[test]
    fn log_level_takes_a_bare_level_or_a_directive() {
        assert_eq!(ok(&["--log-level", "debug", "t.rb"]).log_level.as_deref(), Some("debug"));
        assert_eq!(ok(&["--log-level=info", "t.rb"]).log_level.as_deref(), Some("info"));
        assert_eq!(ok(&["t.rb"]).log_level, None);
        assert!(err(&["--log-level"]).contains("requires a value"));
        // A bare level widens to the whole compiler; a directive is passed
        // through, so one module can still be singled out.
        assert_eq!(widen_bare_level("debug"), "zeo=debug,zeo_rt=debug");
        assert_eq!(widen_bare_level("zeo::analyze=trace"), "zeo::analyze=trace");
        assert_eq!(widen_bare_level("zeo=info,zeo_rt=warn"), "zeo=info,zeo_rt=warn");
    }

    #[test]
    fn emit_clif_takes_an_attached_path_or_stdout() {
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
    fn dump_is_the_ruby_spelling_of_the_inspect_modes() {
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
