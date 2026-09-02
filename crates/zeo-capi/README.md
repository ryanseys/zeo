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

`src/headers/` is what MRI's header tree IS to zeo: the pin
(`ruby-headers.lock`), the edits applied to upstream's `include/` when the
compiler fetches it, and the two files zeo adds. The variadic entries
(`rb_raise`, `rb_sprintf`, `rb_scan_args`, ...) read their `VaList` in Rust,
which is why the crate needs Rust 1.99.
