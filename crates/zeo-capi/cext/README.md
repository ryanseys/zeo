# The C extension surface

zeo compiles a gem's `ext/**/*.c` **from source**, with the system `cc`,
against MRI's own headers. It never loads a prebuilt MRI `.so`.

That is the whole stance, and it has one name: **source-compatible,
ABI-incompatible**. A C extension is C code written against a documented API,
so zeo can answer that API. A `.bundle` built for CRuby is machine code written
against CRuby's object layout, and zeo has no such layout to offer it.

## `include/`

Upstream `ruby/ruby@v4.0.6`'s `include/` tree, verbatim, plus the series in
`patches/`. 195 files -- 194 upstream and one zeo adds,
`ruby/internal/zeo.h`. The pin lives in `ruby-headers.lock`; the tree is
rebuilt from that sum by

```
cargo xtask cext sync            # rewrite include/ from upstream + patches/
cargo xtask cext sync --check    # prove include/ is exactly that sum
cargo xtask cext patch <name>    # record a hand-edit as the next patch
```

`--check` runs in CI. A hand-edit that is not recorded as a patch is drift, and
the check names every file that differs. Every patch opens with a `Subject:`
line and a paragraph saying what it changes; `git apply` ignores both.

## `config/ruby/config.h`

MRI generates this with autoconf, on the machine that builds the interpreter.
zeo has no such step -- it is a compiler, it ships as a binary, and the
extension it builds is built on the user's machine much later. So the file is
written by hand and says only what is true of every target zeo supports:
64-bit macOS and Linux, aarch64 and x86_64, with an `#if` ladder where the two
disagree.

Its include path sits ahead of `include/`, which is how `#include
"ruby/config.h"` finds it. Leaving a `HAVE_` undefined only makes
`ruby/missing.h` declare a fallback prototype; defining one wrongly makes an
extension call a function that is not there, and the failure lands at the end
of a long build. When in doubt, leave it out.

## What the patches change, and what they must not

`VALUE` keeps **MRI's encoding, bit for bit** — `Qfalse`, `Qnil`, `Qtrue`,
`Qundef`, the Fixnum shift, the flonum rotate, the static-Symbol form. An
extension that tests `RB_FIXNUM_P` or writes `INT2FIX` by hand is common and
correct, and it keeps working. Those headers stay unpatched.

A heap object is a `*const Handle`: 8-aligned and non-zero, so
`RB_SPECIAL_CONST_P` is also right unpatched, and canonical per object, so
`a == b` on two `VALUE`s is object identity as it is in MRI.

What a handle does **not** have is a `struct RString` behind it. One rule
answers that, and it covers all eight payload structs:

> A payload struct carries **upstream's layout**. `X(obj)` is a **call**, not
> a cast, and answers a **view** the runtime owns. The view **aliases** real
> storage wherever zeo owns that storage as C-shaped memory, and is
> **refilled** from the object on every reach wherever it does not. A field
> zeo has no answer for is zero, and the accessor named for it raises.

`ruby/internal/zeo.h` declares the eight entries and states the rule;
`crates/zeo-capi/src/view.rs` is the runtime half.

**Aliased.** `RData` and `RTypedData` share one cell, which lives inside the
`CData` itself -- MRI puts `data` at the same offset in both structs and
static-asserts as much. So `RTYPEDDATA(o)->data = p` writes the object's one
slot and cannot go stale, which is what date's `d_lite_marshal_load` needs
after a `ruby_xrealloc`.

**Refilled.** `RString`, `RArray`, `RObject`, `RMatch`, `RRegexp` and `RFile`
are minted beside the object and refilled on every reach, then dropped at the
scope pop. A pointer held across a call back into Ruby therefore reads what
the object looked like at the reach -- MRI gives the same warning about its
own `RSTRING_PTR`. `rb_io_t` is the exception and lives as long as the IO,
because in MRI it IS the IO's own struct and an extension may hold one.

Because the layouts stay upstream's, most accessors are unpatched code:
`RSTRING_PTR/LEN/END`, `RARRAY_LEN/AREF/CONST_PTR/PTR`, `ROBJECT_FIELDS`,
`RREGEXP_SRC`, `RMATCH_REGS`, `DATA_PTR`, `RTYPEDDATA_DATA/_TYPE/_GET_DATA/
_EMBEDDED_P` and MRI's own `offsetof` static assert. Three macros change
beyond the cast: `RARRAY_ASET` calls in (the pointer `RARRAY_PTR_USE` hands
out is a projection, and a store through it would stay there), `RMATCH_EXT`
walks off the view rather than off the `VALUE`, and `RREGEXP_PTR` raises.

Three shape flags make that work: zeo sets `RSTRING_NOEMBED` and
`ROBJECT_HEAP` on an object and never sets `RARRAY_EMBED_FLAG`, so upstream's
own arms take the `as.heap` path a view fills.

The patches are the delta the project maintains by hand. Keep each one to one
subject, and keep the reason in the patch's own header rather than in a
comment inside a vendored file. Keeping the struct bodies is what keeps the
delta small: it is eight macro definitions and eight include lines, so an
upstream bump has almost no context to conflict with.

**The Rust mirrors are generated, not retyped.** `build.rs` runs bindgen over
`cext/probe/mirror.h` into `$OUT_DIR/cext_layout.rs`, with `offset_of`
assertions, so a header bump that moves a field fails the build by field
name. A hand-written `#[repr(C)]` copy would be a second owner of one fact.

## What the tree is measured against

`crates/zeo-rt/tests/cext_headers.rs` compiles `probe/layout.c`, which calls
every macro the series rewrote, both `DATA_PTR(o) = p` lvalue idioms included.
It also asserts the eight payload structs stayed incomplete.

Beyond the probe, the 23 C-extension gems in the oracle's gemdir -- 165k lines
of C -- were compiled with `-fsyntax-only` against this tree and against
pristine MRI 4.0.6 headers, and the two results diffed. **The direct payload
reads all compile now**, and each is a line in `probe/layout.c`:

| Site | Reads | State |
|---|---|---|
| `date/date_core.c:7615` | `RTYPEDDATA(self)->data = dat` | builds; the aliased cell takes the store |
| `io-console/console.c` | `RFILE(io)->fptr` | builds; the `rb_io` view |
| `strscan/strscan.c` (×3) | `RREGEXP(re)->usecnt` | builds -- and mkmf compiles the shim out now anyway (`shims/mkmf_zeo.rb`) |

`RREGEXP(re)->ptr` is the one field left out: `RREGEXP_PTR` raises rather
than hand out a compiled pattern zeo's own engine may recompile.

Measured end to end: `zeo gem install date` succeeds, native build included.
A built `.bundle` RUNNING is proved separately, by the `.cext` goldens below
and by io-console's real extension under `ZEO_DISABLE_BUILTIN`. `require
"date"` is not that proof: it resolves to zeo's own Rust `date` extension by
design.

## Known divergences

The rows below are the eleven places zeo's answer differs from MRI's for an
extension that has loaded.

None of them is a gap file YET, and the reason is now only that each needs its
own small `.c`. The loader has not been the obstacle for some time: `require
"foo"` on a store gem with an `extensions` entry runs its `extconf.rb`,
compiles and links the `.c` out of tree, and calls `Init_foo`.
`crates/zeo/src/parse/loader/cext.rs` is that path and
`crates/zeo/tests/e2e/cext_build.rs` gates the build half of it.

The instrument exists too. A golden carries a `.cext` sidecar naming an
extension directory under `tests/cext/`, and the extension is built TWICE:
`tests/harness/golden.rs` builds it against zeo's vendored headers for the
zeo side, and `cargo xtask bless` builds it against the oracle ruby's own
headers, through its mkmf and `make`, for the recorded answer. Two builds and
not one, because an extension is linked against a runtime -- the oracle
cannot load a bundle built for zeo, and zeo cannot load one built for MRI's
ABI. A `.cext` golden must therefore be blessed; the harness refuses one with
no committed `.expected`, because the live-oracle path would compare zeo
against a ruby raising `LoadError` and a gap would pass proving nothing.

The first row it caught is FIXED rather than filed. `dup` on a `T_DATA`
object used to hand back a copy whose `DATA_PTR` was NULL; it now runs the
class's allocator, which is what CRuby does -- and which is also the safe
answer, because a shallow copy would alias the struct and `dfree` would run
twice on one pointer. Writing that gap found something wider on the way in:
`Klass.new` ignored the allocator `rb_define_alloc_func` registered, though
`Klass.allocate` honoured it, so the ordinary construction path of every
TypedData extension raised before `dup` was reached. Both are closed, and
`tests/a_dup_of_a_c_data_object.rb` is an ordinary passing golden.

The rows below that are BUGS rather than decided divergences become gap files
the same way, one at a time, since each needs its own small `.c`.

| Divergence | Why |
|---|---|
| A cycle closed through a C struct is never reclaimed | A `dmark` enumerates edges and cannot clear one, so `CData::gc_visit` reports on the walk and nothing on the sweep. The asymmetry rule makes this the safe direction: an omitted edge leaks, a reported one that cannot be released can clear a live object. A cycle that merely passes THROUGH a `T_DATA` object is still reclaimed, at its Ruby links. |
| `RB_FLONUM_P` is true for the same doubles as MRI, but an `Integer` outside the Fixnum range is a fresh handle each time | Which is what CRuby does with a Bignum too, so two equal ones are correctly not `equal?`. |
| A store through a refilled view does not reach the object | `RSTRING(s)->len = 3`, `ROBJECT_FIELDS(o)[0] = v` and `fp->fd = n` all change the view alone. The one exception is the BYTES `RSTRING_PTR` answers: those are the String's own, and a write through them is written back at the scope pop. Nothing in the 23-gem census stores through any of the others. |
| `RREGEXP_PTR` raises `NotImplementedError` | The compiled pattern belongs to zeo's regexp engine, which may recompile it, so handing the pointer out would let an extension call onig against a buffer zeo owns. `rb_reg_prepare_re` -- MRI's supported way to get one -- refuses for the same reason. `RREGEXP(re)->ptr` is zero in the view. |
| `rb_scan_args` does not fill the `&block` slot | The block is not in `argv`, and zeo's C method frame carries it separately. The slot is set to `Qnil`; `rb_block_given_p` and `rb_yield` are the working spellings. |
| `ST_DELETE` from an `rb_hash_foreach` callback is not honoured | Deleting under an iteration is a shape zeo's hash does not support, and answering "deleted" without deleting would be worse. `ST_CONTINUE` and `ST_STOP` both work, and the walk runs over a snapshot so the callback may touch the hash. |
| `rb_str_resize` pads with NUL and truncates, and does not preserve capacity | zeo's strings have no separate capacity to preserve. |
| A `rb_encoding *` is an opaque token, not an `OnigEncodingType` | It is `EncodingId + 1` cast to a pointer, so binary (id 0) is not NULL. Every census use passes it around rather than reading it. An extension that dereferences one faults at the read rather than getting a wrong byte. |
| `rb_enc_interned_str` builds an ordinary frozen String | zeo does not intern strings. From the caller's side an interned string IS a frozen one, minus the sharing. |
| The `st_*` hash table is stubbed | Only `st_strcasecmp` and `st_strncasecmp` are implemented, and they are 8 of the 13 `st_*` uses across the 23 gems -- neither touches the table. MRI's `st.c` is 3,224 lines and pulls in five internal headers; vendoring it to serve the other five uses is not a trade worth making. |

## The variadic entries

`rb_funcall(recv, mid, 3, a, b, c)`, `rb_raise(exc, "%s: %d", s, n)`,
`rb_scan_args(argc, argv, "11", &a, &b)`, `rb_rescue2(..., cls, 0)` and the
three variadic `Struct` entries take arguments whose count and types are
described only at run time. Rust cannot read a `va_list` -- there is no stable
way to, and guessing is how a pointer gets read out of the wrong register.

So all of them live in `csrc/cext_va.c`, where `<stdarg.h>` means what it
says. Each one does exactly one thing: pack the varargs into an array (or, for
`rb_raise`, run `vsnprintf` over them) and hand that to the Rust entry that
does the real work. Nothing in that file allocates from the Ruby heap, calls
back into Ruby, or holds a `VALUE` past its own frame.

That is why `rb_raise` formats its message properly and `rb_rescue2` reads its
real class list, rather than each carrying a divergence.
