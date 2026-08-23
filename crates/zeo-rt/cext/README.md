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
`ruby/internal/zeo.h`. The pin lives in `upstream.rb`; the tree is rebuilt
from that sum by

```
tools/zeo-dev cext sync            # rewrite include/ from upstream + patches/
tools/zeo-dev cext sync --check    # prove include/ is exactly that sum
tools/zeo-dev cext patch <name>    # record a hand-edit as the next patch
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

What a handle does **not** have is a `struct RString` behind it. So every
header that reads object layout -- `RSTRING_PTR`, `RARRAY_AREF`, `DATA_PTR`,
`TypedData_Get_Struct` -- is patched to call the runtime instead. That is the
TruffleRuby model. TruffleRuby patches 35 of 194 headers; zeo needs 9, because
the `RBasic` prefix pays for `rbasic.h`, `fl_type.h` and `value_type.h`, and
because `rstruct.h`, `rhash.h` and `rclass.h` were already all function calls
upstream.

The patches are the delta the project maintains by hand. Keep each one to one
subject, and keep the reason in the patch's own header rather than in a
comment inside a vendored file.

## What the tree is measured against

`crates/zeo-rt/tests/cext_headers.rs` compiles `probe/layout.c`, which calls
every macro the series rewrote, both `DATA_PTR(o) = p` lvalue idioms included.
It also asserts the eight payload structs stayed incomplete.

Beyond the probe, the 23 C-extension gems in the oracle's gemdir -- 165k lines
of C -- were compiled with `-fsyntax-only` against this tree and against
pristine MRI 4.0.6 headers, and the two results diffed. Four files differ, in
eight places, and every one is a direct payload read that upstream's headers
would have answered with a byte zeo does not own:

| Site | Reads |
|---|---|
| `date/date_core.c` | `RTYPEDDATA(v)->data` |
| `nio4r/monitor.c`, `nio4r/bytebuffer.c` (×3) | `RFILE(v)->fptr` |
| `strscan/strscan.c` (×3) | `RREGEXP(re)->usecnt`, inside a `#ifndef HAVE_RB_REG_ONIG_MATCH` shim mkmf skips |

Each is a compile error naming its line. That is the design: loud at the call,
never a wrong answer.

## Known divergences

A C extension cannot load yet, so none of these is a gap file: a gap is a
runnable Ruby program, and there is nothing to run. They move to
`tests/gaps/` as the loader lands.

| Divergence | Why |
|---|---|
| `dup` on a `T_DATA` object gives a copy whose `DATA_PTR` is NULL | CRuby calls the class's allocator and copies into a fresh zeroed struct. zeo has no allocator table until `rb_define_alloc_func`. A shallow copy is not an option: two objects sharing one pointer means `dfree` runs twice on it. |
| A cycle closed through a C struct is never reclaimed | A `dmark` enumerates edges and cannot clear one, so `CData::gc_visit` reports on the walk and nothing on the sweep. The asymmetry rule makes this the safe direction: an omitted edge leaks, a reported one that cannot be released can clear a live object. A cycle that merely passes THROUGH a `T_DATA` object is still reclaimed, at its Ruby links. |
| `RB_FLONUM_P` is true for the same doubles as MRI, but an `Integer` outside the Fixnum range is a fresh handle each time | Which is what CRuby does with a Bignum too, so two equal ones are correctly not `equal?`. |
| `ROBJECT_FIELDS` raises `NotImplementedError` | There is no ivar array to hand out. Nothing in the 23-gem census calls it. |
| `RMATCH_EXT`, `RREGEXP(re)->usecnt`, `RFILE(v)->fptr` and `RTYPEDDATA(v)->data` are compile errors | The payload structs are opaque. Each is a direct layout read; see the table above. |
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
