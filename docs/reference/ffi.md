# The C API and FFI surface

What an extension sees when it is built and run by zeo: where the C API
answers differently from MRI's, how `dlopen` resolves, and the whole of the
`ffi` gem API zeo implements.

[Build a C-extension gem](../how-to/build-a-c-extension-gem.md) is the task
this page is the reference for.

## Contents

- [Divergences worth knowing before you debug one](#divergences-worth-knowing-before-you-debug-one)
- [`dlopen` resolves eagerly](#dlopen-resolves-eagerly)
- [FFI: the real `ffi` gem, compiled](#ffi-the-real-ffi-gem-compiled)

## Divergences worth knowing before you debug one

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

## `dlopen` resolves eagerly

`RTLD_NOW`, where MRI uses `RTLD_LAZY`. MRI has no completeness promise to
keep; zeo does, and lazily a symbol it does not export binds to nothing and
faults at the call -- with no symbol name, no backtrace, and no way to tell a
zeo gap from a bug in the gem. Eagerly the same gap is a `LoadError` naming
the symbol.

## FFI: the real `ffi` gem, compiled

Zeo implements the **real `ffi` gem API**, not a custom DSL, so a program
using it runs identically under CRuby+ffi and Zeo. `require
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
`ffi 1.17.4` — see `test/stdlib/ffi/ffi_libc.rb`, `test/stdlib/ffi/ffi_memory.rb`,
`test/stdlib/ffi/ffi_struct.rb`.

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
are all oracle-matched against the C extension (`test/lang/programs/fiddle.rb`); the
`Importer` DSL (`fiddle/import`/`fiddle/struct`) is not included — see
`docs/reference/compatibility.md` `### fiddle` for that and the other divergences.
