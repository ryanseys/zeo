# zeo-capi

MRI's C extension API for [zeo](https://github.com/ryanseys/zeo), written
in Rust over the `zeo-rt` runtime.

A gem's `ext/**/*.c` compiles against MRI's own headers and links against
the `rb_*` entry points this crate exports. zeo is source-compatible with
MRI and ABI-incompatible with it: a heap `VALUE` is an opaque handle, every
layout-reading macro is a call, and a prebuilt MRI `.so` never loads.

The crate installs itself into the runtime through one distributed-slice
element (`zeo_rt::capi_hooks::CAPI_HOOKS`). A `zeo` built without it still
compiles every program; a `require` of a compiled extension then answers
`LoadError` naming the missing support.

`cext/` carries the vendored header tree and `csrc/` the few entries that
still need C; both are on their way out (see `docs/LIMITATIONS.md`).
