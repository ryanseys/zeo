# The Cranelift backend

How a Ruby program becomes machine code, and where to look when it comes
out wrong.

Zeo's front end — parse, lower, analyze — produces HIR plus the analysis
that decides what is statically knowable about it. The **backend** turns
that into something a CPU runs. There are two of them, and only one is the
product.

| Mode | What it does | When |
|---|---|---|
| `jit` | lowers HIR to Cranelift IR and runs it **in this process** | `zeo file.rb`, `zeo -e` |
| `aot` | lowers the same IR to an object file and links a binary | `zeo -o out file.rb`, `--compile` |
| `rustc` | writes Rust text and shells `rustc` | `--backend rustc`, dev tree only |

`jit` and `aot` share **one** lowering (`crates/zeo/src/clif/`) and one
runtime. The only difference between them is `is_pic`, how imported
symbols resolve, and where data lands — so a JIT run and a linked binary
execute the same code. AOT is what ships; the JIT exists because it makes
the dev loop and the test corpus fast (~21 ms for `zeo -e 'puts 1'`), and
because a compiler that can run in-process is what lets `eval` compile for
real later.

`rustc` is the **differential oracle**: the original emitter, frozen, kept
because "the two backends disagree" is a far better bug report than "the
output looks wrong". It is not built for anyone's use — it needs this
repo's own cargo target dir, it is uncached, and it pays a real `rustc`
(~13 s for hello). See [CONTRIBUTING.md](../CONTRIBUTING.md) for how to
run a suite through it.

## The layers

```
HIR + Analyzed
      |
      v
clif/            lowering: HIR -> cranelift_codegen::ir::Function
      |
      +-- backend/jit.rs      -> JITModule       -> call it
      +-- backend/object.rs   -> ObjectModule    -> .o
                                                    |
                                  backend/link.rs -+-> cc + libzeo.a -> binary
```

Emitted code never contains an implementation of Ruby. It contains
control flow, calls, and the small set of things worth inlining
(integer/float arithmetic, tag tests, slot ivar access). Everything else
is a call into `libzeo.a` — the runtime, `crates/zeo-rt`, reached through
the C ABI in `crates/zeo-rt/src/capi/`.

## The contract between the two halves

`crates/zeo-abi/src/abi.rs` is the whole agreement, and both sides assert
it rather than assume it.

- **`RubyValue` is 24 bytes, `#[repr(C, u8)]`**, tag at offset 0 and every
  payload at offset 8. Tags below 16 are immediates: copying one is a
  24-byte memcpy with no refcount traffic. Emitted code reads and writes
  the tag and the `i64`/`f64`/`i8`/`u32` payloads directly and treats
  every heap payload as opaque.
- **Every compiled function returns `i32`.** `0` means the `out` pointer
  holds a value; `1` means a signal is pending, and the emitted code asks
  the runtime what it was (`zeo_rt_signal_kind`, `_take`, `_set`,
  `_save`/`_restore`). This is the explicit form of what the Rust emitter
  spells `Result<RubyValue, Signal>` and `?`.
- **Registration is data, not code.** A program's classes, method rows,
  visibility verbs, reflection metadata, feature units, coverage tables
  and everything else `main` used to call one by one are `#[repr(C)]`
  tables in `.rodata`, pointed at by one `ProgramDesc`. The emitted C
  `main` hands it to `zeo_rt_main`, which walks it in the order the old
  emitted `main` used.
- Row structs are serialized with `offset_of!`/`size_of` from `zeo_abi`
  itself, so the two sides cannot disagree about a field offset.

## Ownership

There is no `Drop` here, so the lowering has to say when a value dies. It
does that with **frame-scoped release pools**: a heap temporary is moved
into the frame's pool at creation and released when the frame pops; a loop
drains its pool at every latch so a long loop does not accumulate; a local
is an owned 24-byte slot released once at function exit. What that buys is
**one landing block per function** instead of a precise per-path owned set
— plus the few landings that must inspect the signal (a block call
catching `break`, a loop forwarding `next`/`redo`, a method folding a
`return` aimed at itself).

Three things check it:

- `clif/verify.rs` — a per-site ownership ledger; a violation is an
  internal compiler error naming the HIR node. On in debug builds,
  `ZEO_CLIF_VERIFY=1` otherwise.
- `ZEO_RT_LEAKCHECK=1` — per-tag live counts across the boundary, plus
  poison-on-release (`0xFF` into the tag byte), so a use-after-release
  aborts at the site instead of corrupting a later value.
- valgrind, on the Linux leg (`scripts/linux/verify.sh valgrind`).

Pool retention is the known cost: a temporary lives to the end of its
frame, which is observable when a program watches `ObjectSpace::WeakMap`.
The long-lived frames (top level, a feature unit) drain at every statement
boundary for exactly that reason.

## Linking

`libzeo.a` is a plain cargo artifact — the `zeo` package builds as both an
rlib and a staticlib, so one `cargo build` produces the `zeo` binary and
the archive AOT programs link, side by side in `target/<profile>/`.
Nothing shells cargo to produce it; if it is missing, the fix is `cargo
build`.

The link line (`backend/link.rs`) has two halves that both fail silently
if they regress, so both are asserted by `e2e/linkage.rs`:

- **whole-archive** (`-force_load` / `--whole-archive`), because linkme's
  `BUILTIN_TABLES` elements live in archive members nothing references by
  name. Lose it and the program still links and runs — it just answers
  `NoMethodError` for whatever went missing.
- **dead-strip** (`-dead_strip` / `--gc-sections`), because that same
  archive carries the compiler as well as the runtime. Lose it and nothing
  fails; the binary just doubles.

The system libraries each target needs are a table in `link.rs`, diffed
against `rustc --print=native-static-libs` by an `#[ignore]`d test that
the Linux leg runs.

## Debug info

`zeo -g file.rb -o prog` (or `ZEO_DEBUGINFO=1`) puts DWARF line tables in
the emitted object, built from the same statement boundaries
`zeo_rt_set_line` marks — so what a debugger says and what `caller` says
cannot drift apart. Zeo's own backtraces never read it; this is for
`lldb`, `perf` and Instruments.

The addresses in it are relocations against the section that defines each
function, never numbers: the linker decides where a function lands, and a
debug reference against a local function SYMBOL is a shape `dsymutil`
does not recognise — it applies no relocation, says nothing about it, and
every row silently keeps the raw offset it was written with.

Platforms differ in where the DWARF ends up, so `-g` changes the link:

- **ELF** links the debug sections straight into the binary.
- **Mach-O** does not. The linker leaves a debug MAP behind — local stab
  entries naming the object file each function came from — so a `-g`
  build keeps `<output>.o` beside the binary and drops the `-Wl,-x` that
  would strip the map. Run `dsymutil <output>` to fold the DWARF into a
  `.dSYM`; lldb and Instruments read it from there.

## Debugging a miscompile

| Tool | Shows |
|---|---|
| `zeo --emit-clif[=<path>] file.rb` | the Cranelift IR, per function |
| `ZEO_CLIF_VERIFY=1` | Cranelift's verifier + the ownership ledger in a release build |
| `zeo --backend rustc file.rb` | what the frozen emitter does with the same HIR |
| `ZEO_RT_LEAKCHECK=1` | ownership, per tag, with poison |
| `cargo nextest run -p zeo` | the CLIF snapshots — the only thing that sees emitter SHAPE |

That last row is a standing rule: **a change under `clif/` runs `cargo
nextest run -p zeo`.** The golden corpora compare what a program printed,
which is blind to the IR that printed it, and insta stops at the first
stale snapshot — so three of them can sit stale behind one report.

## What is not done

- The compiled→compiled call path routes every rich signature (optionals,
  keywords, splats) through a general trampoline and the runtime's
  `bind_params`. Semantics are identical to static routing; the static
  version is a perf lever, not a correctness item.
- FFI sends every call through the runtime's libffi tier. A direct
  `call_indirect` on a declared C signature is the lever.
- The line table is the only DWARF emitted. A debugger names a compiled
  frame and its `file.rb:line`, but has nothing to say about a local
  variable — zeo's values are 24-byte slots with no described type.
- macOS gets no unwind tables (cranelift-object cannot emit Mach-O ones).
  Nothing depends on unwinding: a panic prints and ends the process, and
  fiber teardown is a `Terminate` signal, not a native unwind.
