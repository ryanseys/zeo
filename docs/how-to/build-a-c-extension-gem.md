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
cext hunks --check` proves every edit still applies to the pin. Without the
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
of `zeo` (on by default; `cext` is the older spelling of the same feature).
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

`crates/zeo-capi/src/api.rs` is the census: every symbol an extension can link
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

### Divergences worth knowing before you debug one

* **`RSTRING_PTR` pins.** A zeo String's bytes are a `Vec` behind a lock with
  no stable address, so the pointer is into a copy. The copy lives as long as
  the STRING does -- an extension may keep the pointer across C calls, which
  is what `msgpack`'s `feed_reference` needs and what MRI allows. What
  differs: a write through the pointer lands on the Ruby string at the next
  scope pop, so an extension that writes and then reads the string *through
  Ruby in the same C call* sees the old bytes. A Ruby-side mutation refreshes
  the copy at the next `RSTRING_PTR`, and a length change moves the address --
  MRI's moves on a resize too.
* **A Class handle is immortal.** Every `Init_` stores `cFoo` in a C static
  that outlives the call, and MRI can do that because a class is never
  collected. zeo keeps the handle for the process, which is one box per class
  an extension touches. Anything ELSE stored past its scope still needs
  `rb_gc_register_address`, exactly as on MRI.
* **`rb_thread_call_without_gvl`'s `ubf` is never called.** Interrupting
  opaque C means knowing what it is blocked on. A thread inside a C call is
  not killable until the call returns.
* **`rb_frame_this_func` and `rb_frame_callee` answer the same thing.** MRI
  separates the defined name from the called one, and an alias is what
  separates them; zeo's frame carries one label.
* **`RREGEXP_PTR` and `RMATCH_REGS` are refused.** Both reach into onig
  structures zeo's own engine owns, and handing out a pointer zeo may
  recompile behind would be worse than saying no. `Regexp` and `MatchData`
  answer the same questions through their methods.

### `dlopen` resolves eagerly

`RTLD_NOW`, where MRI uses `RTLD_LAZY`. MRI has no completeness promise to
keep; zeo does, and lazily a symbol it does not export binds to nothing and
faults at the call -- with no symbol name, no backtrace, and no way to tell a
zeo gap from a bug in the gem. Eagerly the same gap is a `LoadError` naming
the symbol.

## FFI — the real `ffi` gem, AOT-compiled

Zeo implements the **real `ffi` gem API**, not a custom DSL, so a program
using it runs identically under CRuby+ffi and Zeo (the north star). `require
"ffi"` is a native no-op; `extend FFI::Library` marks a module; `ffi_lib` and
`attach_function` are recognized at **compile time** and each becomes a
wrapper method whose body is emitted as machine code. The C symbol is
resolved once per call site (`dlopen`/`dlsym`, the gem's own binding) into a
word the wrapper reads on every later call. Two call tiers, one behaviour:
a fixed signature over plain C scalars is a **direct** `call_indirect` on the
declared C signature, with each argument converted through one runtime row
(the gem's range checks and error texts) and the result wrapped inline;
anything Cranelift cannot express — an enum, a callback, a by-value struct,
`:strptr`, varargs, `blocking:` — goes through the runtime's **libffi**
engine from one `.rodata` signature descriptor. Both the plain
`attach_function :name, [args], ret` and the 4-arg rename form `:ruby_name,
:c_name, [args], ret` are supported.

**Types:** `:void`, the integer family (`:char`/`:short`/`:int`/`:long` and the
fixed-width `:int8`…`:int64`), their unsigned twins and `:size_t`,
`:float`/`:double`, `:bool`, `:string` (a `const char *` — a NUL-terminated copy
in, a Ruby String out), and `:pointer` (below). A wrong argument type is a
`TypeError`, exactly as the gem raises. Verified byte-for-byte against
`ffi 1.17.4` — see `tests/ffi_libc.rb`, `tests/ffi_memory.rb`,
`tests/ffi_struct.rb`.

**Memory — `FFI::Pointer` / `FFI::MemoryPointer`.** Real runtime classes over a
native heap buffer. `MemoryPointer.new(:int, 3)` / `.new(bytes)` /
`.from_string(s)` allocate; the full typed accessor surface is present —
`read_/write_` (offset 0) and `get_/put_` (at an offset) for `int8`…`int64`,
their unsigned twins, `float`/`double`, `pointer`, `string`, and raw `bytes`,
plus `read_array_of_int`/`write_array_of_int` (& `long`/`double`), pointer
arithmetic (`ptr + n`), `null?`, `address`, `size`. Bounds-checked (`IndexError`
past an owned buffer). A `:pointer` argument passes the raw address; a `:pointer`
return wraps back to an `FFI::Pointer`.

**`typedef` / `enum`.** `typedef :existing, :alias` (compile-time type alias);
`enum :tag, [:a, 0, :b, ...]` as an `attach_function` type — a Symbol marshals to
its int, an int return maps back to its Symbol (unmapped → Integer), with
auto-increment.

**`FFI::Struct` + `layout`.** `class T < FFI::Struct; layout :f, :type, …; end`.
The `layout` is recognized at compile time; `[]`/`[]=`/`size`/`offset_of`/
`members` are synthesized over an owned `FFI::MemoryPointer` with C field offsets
and alignment. A struct auto-converts to its pointer when passed to a C
`:pointer` argument.

**`callback` and `varargs`.** Both are implemented over a *runtime* C-call
builder (libffi, the `ext-ffi` cargo feature). `callback :tag, [args], ret`
registers a C function-pointer type, so a Ruby `Proc` passed for a `:tag`
argument is marshaled into a libffi closure; a `:varargs` marker in an
`attach_function` type list (`[:string, :varargs]`) builds the variadic call
interface per runtime call, since its trailing arity/types aren't known at
compile time. These are the one part of the gem that the pure
compile-time-`extern` model can't reach without libffi, hence the extra dep.

**The runtime object tier — `Type`, `DynamicLibrary`, `Function`,
`VariadicInvoker`, `FFI.errno`.** The same libffi machinery is also exposed
as the gem's own runtime classes, so code that treats a C call as *data*
(fiddle's pure-Ruby FFI backend is the consumer) runs unchanged:
`FFI::Type::Builtin::*` are the canonical type objects (`#size`/`#alignment`
drive fiddle's whole `SIZEOF_*`/`ALIGN_*` table); `FFI::DynamicLibrary.open`
is `dlopen(3)` (`nil` = the process image) with `#find_function` over
`dlsym`; `FFI::Function.new(ret, args, ptr_or_proc)` builds a callable
function pointer at runtime — from a code address, or from a `Proc` (a
libffi closure, so the object doubles as a C callback) — and IS an
`FFI::Pointer` (`Function < Pointer`, as in the gem);
`FFI::VariadicInvoker#call` marshals trailing `(type, value)` pairs with the
C default argument promotions. The gem's Ruby half lives in `ext/ffi/`
(`Error`/`NullPointerError`, `Platform`, `DataConverter`); every NULL
read/write through a `Pointer` raises `FFI::NullPointerError` rather than
crashing.

**fiddle rides this.** `require "fiddle"` loads `crates/zeo-rt/ext/fiddle/` — fiddle
1.1.8's own `lib/fiddle/ffi_backend.rb` (its JRuby/TruffleRuby path)
vendored over the tier above, plus its `closure`/`function`/`version` files
verbatim. `Fiddle.dlopen`, `Fiddle::Function`, `Fiddle::Pointer`,
`Closure::BlockCaller` callbacks (qsort works), and `TYPE_VARIADIC` calls
are all oracle-matched against the C extension (`tests/fiddle.rb`); the
`Importer` DSL (`fiddle/import`/`fiddle/struct`) is not included — see
`docs/reference/compatibility.md` `### fiddle` for that and the other divergences.
