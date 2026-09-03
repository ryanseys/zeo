# CLI reference

`zeo --help` is the authoritative list; this page is the same thing with room
to explain. Every long option also accepts `--flag=<value>`. An unknown
option is an error, and zeo names the replacement for any spelling it has
removed.

```
usage: zeo [options] (<input.rb> [args...] | -e <code> [--] [args...])
```

Run `zeo --help` for the authoritative list. Every long option also accepts
`--flag=<value>`. An unknown option is an error, and Zeo names the
replacement for any removed spelling.

| Verb | Function |
|---|---|
| `build <input.rb>` | Compile to a binary, do not run. A build has no program `ARGV`, so its options may also follow the file: `zeo build app.rb -o dist/app`. |
| `install [names…]` | Precompile the lockfile's gems into the gem store, so later compiles link them. Never runs Bundler; a gem the package tier cannot carry is reported and keeps compiling from source. |
| `flags [--json]` | Print, shell-quoted on one line, the flags a compile of this project implies — for `$(zeo flags)` in a Makefile. `--json` is the tools' form. |
| `gem <args…>` / `bundle <args…>` | Run the vendored RubyGems / Bundler, no ruby needed. `zeo gem precompile` is zeo's own: build this gem's platform gem with its precompiled artifact inside. |

| Option | Function |
|---|---|
| `<input.rb>` | Compile and run immediately. Option parsing **stops here**, as in ruby: everything after the file name becomes `ARGV`, so `zeo test.rb --seed 42` works. Zeo's own options go **before** the file. |
| `-e <code>` | Compile and run inline code (repeatable; joined with newlines). With `-o`, writes a binary instead. |
| `-o <output>` | Write a native binary here instead of running. |
| `--compile` | Write a native binary at the input path minus its extension. |
| `--package <feature>` | Compile the input as a precompiled package for that require spelling; the artifact is a `.zeopkg` (default name `<feature>.zeopkg`). |
| `--with-package <artifact>` | Link a precompiled package into this program (repeatable). Accepted only on an exact compiler-and-target match; a refused merge drops back to the source compile with a warning. |
| `--link <arg>` | Pass `<arg>` to the `cc` link line as written, after the platform libraries and before the dead-strip flag (repeatable, in order; `--link=<arg>` too). Carries a payload section (`-Wl,-sectcreate,__SEG,__sect,file`), an object file, or `-framework`/`AppKit` as two arguments. A symbol Ruby reaches through `FFI::CURRENT_PROCESS` must be exported by hand (`-Wl,-exported_symbol,_name`) — see [the backend](../explanation/backend.md#linking). Only a linked binary reads these. `ZEO_LINK_ARGS` is the env spelling. |
| `--backend <jit\|aot>` | Pick the output mode. Default: `jit` when running, `aot` with `-o`. `ZEO_BACKEND` is the env spelling. |
| `-I <dir>` | Add a `require` search root (repeatable; `-I<dir>` and `-I=<dir>` too). |
| `--gems <dir>` | Add a directory of vendored gems — each subdirectory with a `.gemspec` is one gem (repeatable). |
| `--root-gem <name>` | Treat this gem as the root package when several provide the same feature (Bundler-root semantics). |
| `--gem-path <dir>` | An installed RubyGems store (`gem env gemdir`). Needs `--bundle-gemfile`. Defaults to `GEM_PATH`. |
| `--bundle-gemfile <path>` | The Gemfile whose lockfile selects versions in the store. Defaults to `BUNDLE_GEMFILE`. |
| `--report[=<path>]` | Write the `zeo-gems.json` disclosure record (default: beside the artifact). Off by default. |
| `--emit-clif[=<path>]` | Print the Cranelift IR and stop. |
| `--dump=<kind>` | Inspect instead of building, then stop. `clif` is `--emit-clif` to stdout; `syntax` prints `Syntax OK` (`-c` is the short spelling); `units` prints the compiled-in load path; `classes` prints every class with the body sites that reveal it. `insns` and `parsetree` are refused. |
| `-w`, `-W[0-2]`, `-W:[no-]<category>` | Accepted in Ruby's shapes. Zeo emits no warnings of its own, so they change nothing. |
| `-v`, `--version`, `-h`, `--help` | Print and stop. |

## `require` search order

The first gem with a given name wins.
 (the first gem with a given name wins):

1. `-I` roots in order, then `RUBYLIB`.
2. `--gems` directories in order.
3. The input file's sibling `gems/` directory.
4. Zeo's own bundled libraries, then the external gem store.

## Environment

Every `ZEO_*` the whole tree reads is in
[Environment variables](environment-variables.md); these are the ones a
compile reads.


| Variable | Function |
|---|---|
| `RUBYOPT` / `RUBYLIB` | As in CRuby. `RUBYOPT` accepts only `-I`, `-w`, `-W`. |
| `GEM_PATH` / `BUNDLE_GEMFILE` | Defaults for `--gem-path` / `--bundle-gemfile`. An ambient store alone never changes a compile. |
| `ZEO_BACKEND` | `jit` or `aot`; the `--backend` flag wins. |
| `ZEO_LINK_ARGS` | Extra `cc` link arguments, whitespace-separated, each as if given by `--link`; they come before the flag's own. |
| `ZEO_CACHE` | `0` turns the compiled-program cache off, so the run compiles from scratch. |
| `ZEO_PROGRAM_CACHE` | Where cached programs live (default: `<build root>/programs`). |
| `ZEO_LOG` / `RUST_LOG` | A `tracing` `EnvFilter` directive, e.g. `zeo::analyze=debug`. Unset means no subscriber and no output. |
| `ZEO_MEMORY_LIMIT` | Bytes of resident memory a compile may use (default: half of RAM, capped at 8 GiB). A breach exits 12 and names the phase. |
| `ZEO_GVL` | `1` runs threads on CRuby's schedule (a FIFO global lock with a 100 ms timer). The default is parallel OS threads. |
| `ZEO_GC` | `1` arms the cycle collector — see [Limitations](limitations.md). |
