# Runtime `eval` and `binding`

**Every `eval` is a run-time compile.** A literal string and a string built
from I/O take the same path: the snippet goes through the same front end, the
same `analyze`, and the same Cranelift emitter a whole program goes through.

It was not always so. A literal string used to be recognized at compile time
and spliced into the HIR arena as `HirNode::Eval`, the way `require` splices a
file's contents. That path was deleted on 2026-08-21 along with the prism
interpreter behind it. One path is worth the run-time cost: a splice and a
compile answered differently often enough that the second implementation was
the bug, and a snippet built from I/O could never use the splice anyway.

## One snippet, one compile (`crates/zeo/src/eval.rs`, `clif/eval.rs`)

The runtime must never depend on the compiler, so `zeo_rt::eval` declares the
seam — `EvalRequest`, `EvalCompiler`, `install` — and the `zeo` library reaches
down and fills it. A program emits a `ProgramDesc` full of tables the runtime
registers at boot; a snippet emits **one function** and the statics it reads:
an entry `(cells, self, out) -> i32`, the status protocol every compiled
function speaks. One `JITModule` per snippet, kept for the process's life
(CRuby keeps an eval's iseq too), cached by source, box, scope names, mode and
cref.

Everything a program's emitter decides statically, a snippet decides at run
time, because a snippet arrives after all of it has already happened:

- **`self` is dynamic** and its ivars are name-keyed — one compiled snippet may
  be evaluated against any number of receivers.
- **A bare name that IS one of the caller's locals reads that local.** prism
  parsed the snippet alone, so such a name could only arrive as a vcall.
- **Nothing registers.** `CompileMode::Eval` leaves a `def`, a `class`, a
  mixin and every visibility statement to the emitter's run-time arms, which is
  what makes them install at their own document position.
- **The definee is a run-time question.** `zeo_rt_eval_define` consults the
  live `instance_eval`/`class_eval` definee first and the eval's own mode only
  as the fallback — the `def` may be sitting in a proc the eval merely BUILT,
  which something else then runs under an `instance_eval` of its own.
- **The cref is a run-time class chain**, travelling as class ids (the one
  thing compiler and runtime always agreed on), with every static fold standing
  down beside it.

A `class`/`module` written in a snippet mints or reuses its class through the
runtime, and its BODY runs as one more `class_eval` of its own source text —
which is what a class body IS in CRuby: a separate iseq with its own cref and
its own locals, sharing nothing with the scope around it.

### The compiler is absent from a program that cannot eval

`ProgramDesc.eval_install` names `zeo_eval_install` only when
`Hir::uses_runtime_eval` (a receiverless `Kernel#eval`, or a string-form
`instance_eval`/`class_eval`/`module_eval`) says the program can reach one. An
eval-free binary references nothing in the compiler, so `-dead_strip` /
`--gc-sections` drops all of it: `puts 1` links 9,764,552 bytes and
`puts eval("1+1")` links 23,947,944, so the compiler is 14,183,392 of any
program that can reach one. This is work matz's interpreter
cannot do — CRuby always ships its parser because it cannot know in advance
whether a program evals.

A program the scan MISSED — `send(m, src)` with a computed name, which no
static analysis can see — raises the runtime's own `NotImplementedError`
saying the binary carries no compiler. Never silent wrong output.

### What zeo declines

A snippet is held to the same contract a program is: CRuby-identical, or a
`NotImplementedError` naming the shape. What it names today: a `refine` or
`using` (refinements are a compile-time decision, and a snippet's call sites
were decided before the `refine` ran), an `FFI::Library` declaration (analyze
assembles that surface from markers), a `Ruby::Box`, and a bare `super` or
`block_given?` at a snippet's own level (both need the enclosing method's
arguments or block channel, which nothing hands a snippet).

A prism-walking interpreter answered here until 2026-08-21. It was the
differential oracle every widening of the compiled path was measured against,
and it is gone: nothing in the tree implements Ruby twice any more.

## `Binding` — the caller's locals, by reference

A `Binding` is the one place an AOT compiler has to give ground. Every other
Ruby local is a Rust stack slot; a scope that hands its locals to code that
isn't compiled yet needs them addressable by NAME and shared by REFERENCE. The
compiler answers exactly that, and only where it's asked
(`analyze::captures::binding_scope_names`): a scope containing a `binding` call
— or a dynamic `eval`, or a read of `TOPLEVEL_BINDING` — gives **its own**
locals the `Arc<Mutex<RubyValue>>` cell storage class that escaping-block
captures already use, then hands the `(name, cell)` list to
`zeo_rt::binding_new`. Every other scope in the program is untouched: the
deoptimization is per-frame and pay-per-use, which is what "a method calling
`binding` deoptimizes only its own locals" means in practice.

Because the cells are shared rather than snapshotted, mutation flows both ways —
`b.local_variable_set(:x, 5)` is visible to the compiled scope, and a later
`x = 6` there is visible through `b`. A Binding also carries its `self`, its box,
and its lexical class (CRuby's cref), which is what makes `binding.eval("K")`
inside `module M` find `M::K` and a `def` inside it install where CRuby's does.
The scope object is two-tiered exactly as CRuby's environment is: `dup` copies
the Binding but not the environment (a write through the copy reaches the
original's locals), while a local ADDED to the copy stays on the copy.

That makes `eval` faithful in both directions. A receiver-less `eval(src)`
compiles into a Binding of the calling frame, so the source reads and writes the
caller's own locals; new locals it introduces live in a child layer and die with
the call, as CRuby's do. `Binding#eval` and `eval(src, b)` run in the Binding
itself, where new locals persist. So `x = 1; eval("x + 1")` answers `2` because
the snippet's compile takes `x` from the caller's Binding by name, not because
anything was spliced.

`Proc#binding` follows from the same capture. A block literal's construction
site emits a Binding of *its enclosing* scope — the one the block was written
in, whose `self` and locals are what CRuby's answer holds; the block's own
locals are not in it, because they do not exist until it runs. Only a program
that can ask pays: `Hir::uses_proc_binding` looks for a receiver-ful `binding`
call (or a `send(:binding)`) anywhere in the program, and only then does a scope
containing a block literal get cells at all. A proc built without one — a
runtime-internal `Symbol#to_proc`, or any proc in a program that never reflects
that way — answers CRuby's `ArgumentError: Can't create Binding from C level
Proc`.

The reflective spelling works too: `send(:eval, src)` and `obj.send(:eval, src)`
compile exactly like a direct `eval`, taking their locals from the caller's
frame and their `self` from the receiver, and a literal `send(:eval, …)` also
counts as an eval site for the installer scan.

**Bounds.** A `binding` inside an inline-spliced iterator block sees that
block's own parameters only when the enclosing scope is already a binding scope
(the spliced param is otherwise a plain per-iteration slot). `send` with a
COMPUTED method name (`m = :eval; send(m, src)`) is invisible to every static
analysis here: it neither installs the compiler nor gets a scope, and the honest
failure mode is the runtime's own `NotImplementedError` or a `NameError` on the
first caller local. So is `method(:eval).call(src)` — a `Method` carries no
binding.

## Runtime metaprogramming without a compile

Some runtime metaprogramming needs no compile at all: a compiled block is already
an `Arc<dyn Fn>`, so a class or method defined at runtime *from a block* needs
only a runtime-mutable method registry, not an interpreter. That foundation is
`zeo-rt/src/runtime_meta.rs`: a lock-guarded overlay beside the frozen
`OnceLock` registry, gated by a single `is_live()` atomic so programs that
never reach it pay nothing. On it:

- **Runtime `define_method`** (computed name, in a class-body `each` loop) — the
  block becomes a `MethodImpl::Dynamic`. Class-body statements execute, and a
  nested block may capture the enclosing block's own local.
- **Per-object singletons** (`def obj.foo`, `class << obj`,
  `obj.define_singleton_method`) — an identity-keyed overlay table.
- **`Class.new(Super) { … }`** — a runtime class id plus a name-keyed
  `DynObject` instance type.

**Documented boundary (inherent to AOT):** a runtime `define_method` that
*overrides* a method the compiler dispatched *statically* (a direct `Klass::m`
call, not through `send`) is invisible at that call site.

## Non-goals

`TracePoint`, `ObjectSpace`, Ractors, and refinements are all in scope and
implemented in the runtime at large: `TracePoint` (and `set_trace_func`) via
the armed-only tracing machinery in `zeo-rt/src/ext/tracepoint.rs`,
`ObjectSpace` as an always-on builtin (`each_object` alone is a declared
refusal), Ractors as real OS threads with shareability checks, and refinements
both as the compile-time lexical rewrite and as runtime `Module#refine` /
`using Module.new { … }` overlays. The one enduring non-goal here is a
*reflective* eval the static analysis cannot see — a `send(m, str)` whose
method name is computed at runtime hides the eval site from
`uses_runtime_eval`, so its binary may carry no compiler; the honest failure
mode there is the runtime's own `NotImplementedError`, never silent wrong
output.
