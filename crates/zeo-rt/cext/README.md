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
