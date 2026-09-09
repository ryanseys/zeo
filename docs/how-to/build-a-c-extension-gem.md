# Build a C-extension gem

A gem that ships C compiles from source, against MRI's own headers. Nothing
prebuilt is ever loaded: a `.bundle` built against MRI's ABI cannot run in a
zeo program, and pretending otherwise would fail at a call rather than at a
load.

A gem that ships its C as SOURCE is compiled from it. zeo is
**source-compatible with CRuby and ABI-incompatible**: a gem's `ext/**/*.c`
builds against MRI's own headers, and a prebuilt MRI `.so` never loads.

Nothing of that header tree is in the repository. The first extension build
fetches upstream's `include/` at the rev `crates/zeo-capi/ruby-headers.lock`
pins (the GitHub archive, verified by sha256; `git` when `curl` cannot),
applies the edits `zeo-capi/src/headers/hunks.rs` holds -- every macro that
reads object layout becomes a call, because a zeo object is an opaque handle
-- and writes the two files zeo adds, `ruby/internal/zeo.h` and a
`ruby/config.h` rendered for the host. The finished tree lives under the
build root (`target/ruby-headers/<rev>/` in the dev tree, the per-user cache
for an install), a release tarball carries it pre-seeded, and `cargo xtask
check-c-headers hunks --check` proves every edit still applies to the pin. Without the
network, `ZEO_RUBY_HEADERS_TARBALL` names a local copy of the archive and
`ZEO_RUBY_HEADERS_DIR` names the `include/` directory of a `ruby/ruby`
checkout at that rev.

The whole path, per gem:

1. The gemspec's `s.extensions` names an `extconf.rb`.
2. zeo runs it, with `RbConfig::CONFIG` supplied by its own shim and `mkmf`
   vendored from the same pin as the headers. mkmf writes a Makefile.
3. zeo reads that Makefile's variables, then compiles and links from them --
   in parallel and without `make`. A Makefile carrying a rule mkmf did not
   write (a `depend` file, a generated header) goes to real `make` instead;
   `ZEO_CEXT_MAKE=1` forces that path for everything.
4. The program `dlopen`s the result at the `require`'s own line and calls
   `Init_<name>`.

The build is **out of tree**. A gem store is shared and often read only, so
the sources are staged into zeo's cache and built there, keyed by their own
bytes -- a second compile of the same gem finds the shared object and skips
the build.

### What this does not do

* **A precompiled binary gem never loads.** `nokogiri-1.16.0-arm64-darwin`
  ships a `.so` built against CRuby's ABI. Install the ruby-platform variant
  (`bundle config set force_ruby_platform true`) and zeo compiles it.
* **Only a shared object zeo built is dlopened.** zeo writes a `<product>.zeo`
  sidecar beside everything it links, and both loaders (the compile-time
  `-I` root search and the runtime `Kernel#require`) refuse a `.so`/`.bundle`
  without one, naming the file. The load path is full of the other kind --
  `bundle install` leaves the extension it compiled for CRuby beside the
  gem's Ruby (`gems/erb-6.0.7/lib/erb/escape.bundle`) -- and that object
  runs under zeo until its first field read, then faults. A `rescue
  LoadError` around such a require takes the gem's own fallback, as on a
  ruby without the extension.
* **Autotools and `mini_portile` builds of a vendored C library are out of
  scope.** System-library mode through `have_library` works.
* **A C extension and a second Ruby thread arm the GVL**, and turn off the
  lock-free container path for good. A single-threaded program keeps that
  path; it is hidden only while one of the extension's own frames is live.
  The cost is real once a thread exists and it is not optional: the C holds
  Ruby objects no other thread's view accounts for. A thread the extension
  starts through `rb_thread_create` counts; a raw `pthread_create` is outside
  the contract.
* **The shared object has to be there at run time.** An AOT binary dlopens
  it from the cache; it is not linked in. The gem report names the path.

### The C API surface

The API lives in its own crate, `zeo-capi`, behind the `capi` cargo feature
of `zeo`, which is on by default.
The runtime never names the crate: it asks its four questions -- load an
extension, does this class allocate through `rb_define_alloc_func`, run
that allocator, run the `ruby_vm_at_exit` callbacks -- through
`zeo_rt::capi_hooks`, and linking the crate fills those slots. A `zeo` built
`--no-default-features` compiles every program; a `require` of a compiled
extension then answers `LoadError` naming the missing support.

**A raise is a Rust unwind.** `rb_raise` and every other raising entry
start an unwind that carries the exception, and the entry that called into
the extension (`rb_protect`, a method trampoline, `Init_`) catches it and
answers the Ruby exception. The unwind travels through the extension's own
frames, so its objects must carry unwind tables: `RbConfig`'s `cflags` add
`-fexceptions -fasynchronous-unwind-tables`, `mkmf` drops a gem's own
`-fno-` twins with a warning, and zeo refuses to build a Makefile that still
carries one. A C++ extension that wraps a Ruby call in `catch (...)`
intercepts the unwind; `rb_protect` is the spelling for that. A genuine bug
in zeo (a Rust panic) is never turned into a Ruby exception.

`crates/zeo-capi/src/api.rs` is the full list: every symbol an extension can link
against, read off clang's AST of every public header. 33 of them are
REFUSALS, not gaps, and each raises with its reason:

| What | Why |
|---|---|
| 21 `ruby_*` entries | they boot, configure or shut down an interpreter, and an extension loaded INTO a running one cannot |
| 5 `rb_big_*` entries | they expose MRI's Bignum digit array; `rb_integer_pack`/`rb_integer_unpack` are the supported way and both work |
| `rb_hash_tbl`, `rb_hash_bulk_insert_into_st_table` | they hand out a Ruby Hash's internal `st_table` |
| `rb_load_file`, `rb_load_file_str` | they answer a `NODE*`, MRI's parse tree |
| `rb_add_event_hook`, `rb_remove_event_hook` | the C-level TracePoint, whose event set does not line up |
| `rb_marshal_define_compat` | Marshal's internal compatibility table |

### Before you debug a build

Where zeo's C API answers differently from MRI's, and why `dlopen` resolves
eagerly, are in [the C API and FFI surface](../reference/ffi.md). That page
also carries the whole `ffi` gem API zeo implements, which is often the
shorter road to a C library than an extension is.
