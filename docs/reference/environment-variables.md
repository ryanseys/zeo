# Environment variables

Every `ZEO_*` the tree reads, grouped by who reads it. `checks::env_vars`
holds this page to the code in both directions: a variable named here with
no reader fails the suite, and a reader with no row here fails it too.

## A compile

| Variable | Effect |
|---|---|
| `ZEO_BACKEND` | `jit` or `aot`. The `--backend` flag wins. |
| `ZEO_LINK_ARGS` | Extra `cc` link arguments, whitespace-separated, each as if given by `--link`. They come before the flag's own. |
| `ZEO_CACHE` | `0` turns the compiled-program cache off, so the run compiles from scratch. |
| `ZEO_CACHE_DIR` | Where the compiled-program cache lives. |
| `ZEO_PROGRAM_CACHE` | Where cached programs live (default: `<build root>/programs`). |
| `ZEO_PACKAGE_CACHE` | Where auto-built packages live. |
| `ZEO_MEMORY_LIMIT` | Bytes of resident memory a compile may use (default: half of RAM, capped at 8 GiB). A breach exits 12 and names the phase. |
| `ZEO_CODEGEN_THREADS` | How many threads emit CLIF. |
| `ZEO_LOG` / `RUST_LOG` | A `tracing` `EnvFilter` directive, e.g. `zeo::analyze=debug`. Unset means no subscriber and no output. |
| `ZEO_DEBUG` | Comma-separated compiler switches, for bisecting an optimization. |
| `ZEO_DEBUGINFO` | Emit DWARF for a linked binary. |
| `ZEO_CLIF_VERIFY` | Run Cranelift's own verifier over everything emitted. |
| `ZEO_TIMINGS` | Print what each phase cost. |
| `ZEO_HOME` | Where the payload is, overriding the `bin/../share/zeo` probe. |
| `ZEO_PREFIX` | Where `cargo install` puts the payload. |
| `ZEO_DISABLE_BUILTIN` | Comma-separated library names zeo must NOT answer itself, so the store's copy is used instead. |

## A compiled program, at run time

| Variable | Effect |
|---|---|
| `ZEO_GVL` | `1` runs threads on CRuby's schedule (a FIFO global lock with a 100 ms timer). The default is parallel OS threads. |
| `ZEO_GC` | `1` arms the cycle collector, and the allocation registry it needs. |
| `ZEO_THREADS` | The thread-pool size. |
| `ZEO_RT_TRACE` | Trace runtime calls. |
| `ZEO_RT_LEAKCHECK` | The compiled-ownership ledger: a non-zero balance at exit is a leak or a double-consume in the emitted lowering. |
| `ZEO_RT_LEAKTRACE` | Which allocations the ledger is still holding. |
| `ZEO_RT_GCCHECK` | Write the exit cycle census, which the corpus holds to each program's `#@ gccheck` line. |
| `ZEO_RT_GCSTATS` / `ZEO_RT_GCRINGS` | What the collector did, and which rings it found. |
| `ZEO_RT_NO_SOLE_THREAD` | Turn off the sole-thread fast path. |

## Building a gem's C extension

| Variable | Effect |
|---|---|
| `ZEO_RUBY_HEADERS_DIR` | Use these MRI headers instead of fetching the pinned ones. |
| `ZEO_RUBY_HEADERS_TARBALL` | Unpack the headers from this archive instead of fetching. |
| `ZEO_CEXT_HDRDIR` / `ZEO_CEXT_ARCHHDRDIR` | What `mkmf` reports as the header directories. |
| `ZEO_CEXT_MAKE` | The `make` to run for an extension. |
| `ZEO_SOEXT` / `ZEO_DLEXT` | The shared-object extension a built gem gets. |
| `ZEO_BINDIR` / `ZEO_RUBY_INSTALL_NAME` | What the `rbconfig` shim reports as the ruby binary's directory and name, for an `extconf.rb` that shells out to ruby. |
| `ZEO_HOST_OS` / `ZEO_HOST_CPU` / `ZEO_RUBY_PLATFORM` / `ZEO_GEM_PLATFORM` | Override what the build reports about this machine. |

## Development

| Variable | Effect |
|---|---|
| `ZEO_RUBY` | The pinned ruby the oracle is. `.ruby-version` says which version it must be. |
| `ZEO_BIN` | Use this `zeo` instead of building one. |
| `ZEO_BENCH_DIST` | `pgo` benchmarks the shipped configuration. |
| `ZEO_BENCH_ORACLE` / `ZEO_BENCH_ORACLE_RUBY` | Time the pinned ruby beside zeo. |
| `ZEO_BENCH_RESUME` | Continue a benchmark run that was interrupted. |
| `ZEO_LINUX_IMAGE` / `ZEO_LINUX_MEMORY` / `ZEO_LINUX_THREADS` / `ZEO_LINUX_PROFILE` / `ZEO_LINUX_VOLUME` / `ZEO_CONTAINER_ENGINE` | The Linux container loop. |
| `ZEO_ARITY_DEBUG` | Name, on an arity error, which dispatch path bound the call. |
| `ZEO_DEBUG_DROP_TABLE` | Link a program without one class table, for per-table size attribution. |
| `ZEO_DEBUG_RUNTIME_LOAD` | Narrate what the runtime registers at boot. |
| `ZEO_MSPEC_STUBS` | Stub what a vendored spec suite expects and zeo does not have. |
| `ZEO_TEST_*` | Fixtures the `ENV` unit tests set and read; nothing else reads them. |

## Ruby's own

`RUBYOPT` and `RUBYLIB` work as they do in CRuby; `RUBYOPT` accepts only
`-I`, `-w` and `-W`. `GEM_PATH` and `BUNDLE_GEMFILE` are the defaults for
`--gem-path` and `--bundle-gemfile`. An ambient store alone never changes a
compile.

Two more of ruby's own knobs the runtime honours:

| Variable | Effect |
|---|---|
| `RUBY_BOX` | `1` enables `Ruby::Box`. Without it the class raises, as CRuby's does. |
| `RUBY_IO_BUFFER_DEFAULT_SIZE` | The default size of an `IO::Buffer`. |
