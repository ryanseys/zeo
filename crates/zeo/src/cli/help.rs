//! `zeo --help`: the one place the verbs and flags are spelled for a person.

pub(crate) const HELP: &str = "\
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
  flags [--json]        print, on one shell-quoted line, exactly the flags a
                        compile of this project implies (store, Gemfile, and
                        every linkable precompiled package) -- for
                        `$(zeo flags)` in a Makefile. --json prints the
                        structured form for tools instead. One producer with
                        the verbs above, so the handoff cannot drift
  backend <f.clif> -o <bin>
                        link a program from CLIF text another front end
                        wrote (see ze0/). `--data <f.zeodata>` names the
                        sidecar carrying what the text cannot; without it,
                        the `.zeodata` beside the file, else an empty one
                        A script really named `build`, `gem`, `bundle`,
                        `install`, `flags` or `backend` still runs as
                        `zeo ./build`; a verb never depends on what is in
                        the current directory.

options:
  -o <output>           where to write the compiled binary
  --compile             write the default-named binary instead of running
  --emit-clif[=<path>]  emit the Cranelift IR (the aot backend's own
                        lowering) instead of building; bare prints to stdout
  --emit-zeodata=<path> with --emit-clif: also write the sidecar that
                        `zeo backend` reads beside the text
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
                        (`crate::analyze=debug`) narrows it. Beats ZEO_LOG and
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
  --link <arg>          pass <arg> to the `cc` link line as written, after the
                        platform libraries and before the dead-strip flag
                        (repeatable, in order). Carries a payload section
                        (`--link -Wl,-sectcreate,__SEG,__sect,file`), an
                        object file, or `--link -framework --link AppKit`.
                        A symbol Ruby reaches through FFI::CURRENT_PROCESS
                        must be exported by hand
                        (`--link -Wl,-exported_symbol,_name`); zeo never adds
                        -export_dynamic for it. Only a linked binary reads
                        these (ZEO_LINK_ARGS is the env spelling)
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
  ZEO_LINK_ARGS         extra `cc` link arguments, whitespace-separated, each
                        one as if given by --link; they come before the
                        flag's own
  ZEO_LOG / RUST_LOG    a `tracing` EnvFilter directive for zeo's internal
                        logs, e.g. `zeo=debug` or
                        `crate::analyze=debug,crate::lower=trace`. --log-level is
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

/// The help message, plus the `--enable`/`--disable` table rendered from the
/// dials themselves -- so what it says a build starts with is what that build
/// actually starts with.
pub(crate) fn print_help() {
    print!("{HELP}");
    println!("\nfeatures (--enable=<name> / --disable=<name>):");
    for line in crate::cli::features::help_lines() {
        println!("  {line}");
    }
}
