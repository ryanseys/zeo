# The C extension surface

zeo compiles a gem's `ext/**/*.c` **from source**, with the system `cc`,
against MRI's own headers. It never loads a prebuilt MRI `.so`.

That is the whole stance, and it has one name: **source-compatible,
ABI-incompatible**. A C extension is C code written against a documented API,
so zeo can answer that API. A `.bundle` built for CRuby is machine code written
against CRuby's object layout, and zeo has no such layout to offer it.

## `include/`

Upstream `ruby/ruby@v4.0.6`'s `include/` tree, verbatim, plus the series in
`patches/`. 194 files. The pin lives in `upstream.rb`; the tree is rebuilt from
that sum by

```
tools/zeo-dev cext sync            # rewrite include/ from upstream + patches/
tools/zeo-dev cext sync --check    # prove include/ is exactly that sum
tools/zeo-dev cext patch <name>    # record a hand-edit as the next patch
```

`--check` runs in CI. A hand-edit that is not recorded as a patch is drift, and
the check names every file that differs.

## What the patches change, and what they must not

`VALUE` keeps **MRI's encoding, bit for bit** — `Qfalse`, `Qnil`, `Qtrue`,
`Qundef`, the Fixnum shift, the flonum rotate, the static-Symbol form. An
extension that tests `RB_FIXNUM_P` or writes `INT2FIX` by hand is common and
correct, and it keeps working. Those headers stay unpatched.

A heap object is a `*const Handle`: 8-aligned and non-zero, so
`RB_SPECIAL_CONST_P` is also right unpatched, and canonical per object, so
`a == b` on two `VALUE`s is object identity as it is in MRI.

What a handle does **not** have is a `struct RString` behind it. So every
header that reads object layout — `RSTRING_PTR`, `RARRAY_AREF`, `RBASIC`'s
flags, `DATA_PTR`, `TypedData_Get_Struct` — is patched to call the runtime
instead. That is the TruffleRuby model, and its patched-header list is the map:
35 of 194.

The patches are the delta the project maintains by hand. Keep each one to one
subject, and keep the reason in the patch's own header line rather than in a
comment inside a vendored file.
