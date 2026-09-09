# CLI reference

`zeo --help` is the authoritative list; this page is the same thing with room
to explain. Every long option also accepts `--flag=<value>`. An unknown
option is an error, and zeo names the replacement for any spelling it has
removed.

```
usage: zeo [options] (<input.rb> [args...] | -e <code> [--] [args...])
```

## Modes

| Mode | Function |
|---|---|
| `<input.rb> [args…]` | Compile the file and run it immediately, like ruby. Option parsing **stops here**, as in ruby: everything after the file name becomes `ARGV`, so `zeo test.rb --seed 42` works. Zeo's own options go **before** the file. |
| `-e <code>` | Compile and run inline code (repeatable; joined with newlines). Trailing arguments become `ARGV`. With `-o`, writes a binary instead. |
| `-o <path> <input.rb>` | Write a native binary at `<path>` instead of running. |
| `--compile <input.rb>` | Write a native binary at the input path minus its extension. |
| (no arguments) | Open an irb shell when there is a terminal to talk to. `--irb` opens it by name, for a wrapper or a pty. |

## Verbs

| Verb | Function |
|---|---|
| `build <input.rb>` | Compile to a binary, do not run. A build has no program `ARGV`, so its options may also follow the file: `zeo build app.rb -o dist/app`. |
| `gem <args…>` / `bundle <args…>` | Run the vendored RubyGems / Bundler, no ruby needed. `bundle exec` and `bundler/setup` do not work; see [Compatibility](compatibility.md). |
| `install [names…]` | Precompile the lockfile's gems into the gem store, so later compiles link them. Never runs Bundler and never touches the network; run `zeo bundle install` first. A gem the package tier cannot carry is reported and keeps compiling from source. |
| `flags [--json]` | Print, shell-quoted on one line, the flags a compile of this project implies (store, Gemfile, every linkable package), for `$(zeo flags)` in a build script. `--json` is the tools' form. |
| `backend <f.clif> -o <bin>` | Link a program from Cranelift IR written as text by another front end. `--data <f.zeodata>` names the sidecar carrying what the text cannot say; without it, the `.zeodata` beside the file, else an empty one. |

A script really named `build`, `gem`, `bundle`, `install`, `flags` or
`backend` still runs as `zeo ./build`; a verb never depends on what is in the
current directory.

## Options

| Option | Function |
|---|---|
| `-o <output>` | Where to write the compiled binary. |
| `--compile` | Write the default-named binary instead of running. |
| `--emit-clif[=<path>]` | Emit the Cranelift IR instead of building; bare prints to stdout. |
| `--emit-zeodata=<path>` | With `--emit-clif`: also write the sidecar `zeo backend` reads beside the text. |
| `--dump=<kind>` | Inspect instead of building, then stop. `clif` is `--emit-clif` to stdout; `syntax` prints `Syntax OK` (`-c` is the short spelling); `units` prints the compiled-in load path; `classes[=<filter>]` says which class bodies ever run; `methods[=<rows>]` counts what inheritance costs, since one `def` is emitted once per class that carries it. `insns` and `parsetree` are refused: zeo emits no bytecode. |
| `--backend <jit\|aot>` | Pick the output mode. Default: `jit` when running, `aot` with `-o`. `ZEO_BACKEND` is the env spelling. |
| `-g` | Put DWARF line tables in the compiled program, so lldb, perf and Instruments name a Ruby frame by file and line. `ZEO_DEBUGINFO=1` is the env spelling. |
| `--log-level <level>` | Narrate the compiler's own work on stderr: a bare level (`error` … `trace`) or a `tracing` directive (`crate::analyze=debug`). Beats `ZEO_LOG` and `RUST_LOG`. |
| `-I <dir>` | Add a `require` search root (repeatable; `-I<dir>` and `-I=<dir>` too). |
| `-r <library>` | Require a library before the program's first line, like ruby's `-r` (repeatable, in order). |
| `--gems <dir>` | Add a directory of vendored gems — each subdirectory with a `.gemspec` is one gem (repeatable). |
| `--embed-sources <dir>` | Carry this directory's `.rb` files inside the program, so a require only the run time can resolve finds them without a filesystem (repeatable; off by default). |
| `--strict-static-require` | Refuse at compile time a `require`/`load` this compile cannot resolve, instead of leaving it to the run-time loader. |
| `--package <feature>` | Compile the input as a precompiled package for that require spelling; the artifact is a `.zeopkg` (default name `<feature>.zeopkg`). |
| `--with-package <artifact>` | Link a precompiled package into this program (repeatable). Accepted only on an exact compiler, target and interface match; anything else is refused by name. |
| `--link <arg>` | Pass `<arg>` to the `cc` link line as written, after the platform libraries and before the dead-strip flag (repeatable, in order). Carries a payload section (`-Wl,-sectcreate,__SEG,__sect,file`), an object file, or `-framework`/`AppKit` as two arguments. A symbol Ruby reaches through `FFI::CURRENT_PROCESS` must be exported by hand (`-Wl,-exported_symbol,_name`) — see [the backend](../explanation/backend.md). Only a linked binary reads these. `ZEO_LINK_ARGS` is the env spelling. |
| `--root-gem <name>` | Treat this gem as the root package when several provide the same feature (Bundler-root semantics). |
| `--gem-path <dir>` | An installed RubyGems store (`gem env gemdir`) to resolve locked gems against (repeatable). Defaults to `GEM_PATH`. Needs `--bundle-gemfile`. |
| `--bundle-gemfile <path>` | The Gemfile whose lockfile selects versions in the store. Defaults to `BUNDLE_GEMFILE`. Needs `--gem-path`. |
| `--report[=<path>]` | Write the `zeo-gems.json` disclosure record (default: beside the artifact). Off by default. |
| `--enable=<features>` / `--disable=<features>` | Turn on or off what the program starts with, before its own first line: ruby's own names, comma-separated, plus `all`. `--enable-gems` is accepted too. `zeo --help` lists each feature and where this build leaves it. |
| `-w`, `-W[0-2]`, `-W:[no-]<category>` | Accepted in Ruby's shapes. Zeo emits no warnings of its own, so they change nothing. |
| `-v`, `--version`, `-h`, `--help` | Print and stop. |

## `require` search order

The first gem with a given name wins:

1. `-I` roots in order, then `RUBYLIB`.
2. `--gems` directories in order.
3. The input file's sibling `gems/` directory.
4. Zeo's own bundled libraries, then the external gem store.

Step 4 reaches zeo's own copy of the stdlib, not the machine's. Nothing a
`gem install` put in a store is visible to a compile until `--gem-path` and
`--bundle-gemfile` name it, so an installed zeo compiles the same program the
same way on every machine. `--report` writes down which tier answered each
require.

## Environment

`RUBYOPT` (only `-I`, `-w`, `-W`), `RUBYLIB`, `GEM_PATH` and `BUNDLE_GEMFILE`
work as in CRuby, with the defaults the table above states. Every `ZEO_*`
variable is in [Environment variables](environment-variables.md).
