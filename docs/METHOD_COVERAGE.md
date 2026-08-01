# Close the method-coverage gap against CRuby 4.0.6

## Context

The reflection-parity work fixed how zeo *describes* its surface. This plan
fixes the surface itself: the methods CRuby answers and zeo does not.

An earlier figure of "~1,976 missing methods" was wrong. It came from a census
over 38 hand-listed classes that counted inherited methods repeatedly. A
transitive census gives the real numbers: every `Module` reachable from
`Object`'s constant tree, depth 4, keyed by `name`, dumped from both engines and
diffed. CRuby is `mise exec ruby@4.0.6 -- ruby --disable-gems`.

The census records, per module, `instance_methods(false)`,
`singleton_methods(false)`, `private_instance_methods(false)`,
`constants(false)`, and — the dimension the old one lacked — the *inherit-true*
sets. That last pair is what separates "raises NoMethodError" from "works, but
reflection disagrees".

**Modules: CRuby 261, zeo 140.**

| bucket | count | what the user sees |
|---|---|---|
| Unreachable methods on classes zeo HAS | 255 | `NoMethodError` |
| Methods on classes zeo LACKS | 335 | `NameError` on the constant |
| `Errno::*` constants | 135 | `rescue Errno::ERANGE` → `NameError` |
| `module_function` private half | ~110 | `include Process` then `pid` → `NameError` |
| Owner divergences | 157 | works; `.owner` and `super` land elsewhere |
| Enumeration-only | 25 | works, right owner, absent from `instance_methods(false)` |

Plus one defect found while building the census: **`object_id` returns the same
value for every class and module.** `hash`, `equal?`, Hash-keying and `uniq` are
all correct, so this is narrow — but it silently breaks any cache keyed on a
class's `object_id`, and it is what made the first census run stop after two
rows.

Scope is core Ruby with no requires. Stdlib-file compilation (386/728) is a
separate axis and is not part of this plan.

## Deliberately out of scope

Recorded so the count reconciles, each with a reason:

- `RubyVM::*` (67 methods) — MRI bytecode internals. `InstructionSequence`,
  `YJIT` and `AbstractSyntaxTree` describe a VM zeo does not have.
- `Ruby::Box` (15) — 4.0's isolation primitive, tied to the MRI object space.
- `Ractor` beyond the existing stub (~30) — needs a real parallel object space.
- `IO::Buffer` (49) — a genuine feature, but it needs a zero-copy byte-buffer
  object of its own. Deferred, not refused.

## Wave 1 — structural: one mechanism, many methods

### 1.1 `module_function` for `Kernel`, `Process`, `FileTest`, `ObjectSpace`, `Signal`

The single highest-leverage fix. `Math` already does this correctly (the arity
work converted it); the other five still declare `def self.X`, so they get a
singleton copy and no private instance copy.

The observable defect: `class Host; include Process; def go; pid; end; end` →
`NameError`. Same for `FileTest`, which additionally shared *all* of `File`'s
class-method table, so `FileTest.read` answered where CRuby raises.

`Kernel` is a different case and the reflection counts overstate it. Measured
name by name, only `autoload`, `autoload?` and `Pathname` are genuinely
unreachable; the rest of the 62 work as **compiler intrinsics** (`block_given?`,
`binding`, `lambda`, `loop`, `at_exit`, `__method__`, `local_variables`, …) and
are simply absent from the tables reflection walks. `syscall` is missing from
both engines.

A separate defect surfaced here: `5.puts("x")` answers where CRuby raises. The
runtime *does* know the method is private (`respond_to?` → false,
`private_method_defined?` → true, `public_send` raises), but codegen folds an
explicit-receiver builtin call without consulting visibility, because the
compiler's `Surface` records no visibility for a builtin row. Pre-existing and
tracked separately at `tests/gaps/module_function_private_receiver.rb`.

**Ordering constraint**: the singleton copies must land *before* that
enforcement is tightened. `Kernel.puts` currently resolves only because Kernel
is itself an object that includes Kernel and the private check is skipped —
fixing enforcement first would break it.

Exact targets, oracle-measured:

| module | `module_function` pairs | singleton-only | public instance |
|---|---|---|---|
| `Kernel` | 62 | 0 | 43 |
| `Process` | 39 | 8 (`_fork`, `abort`, `exec`, `exit`, `exit!`, `fork`, `last_status`, `spawn`) | 0 |
| `FileTest` | 26 | 0 | 0 |
| `ObjectSpace` | 6 | 0 | 0 |
| `Signal` | 3 | 0 | 0 |

`Kernel` also wants 6 private-only rows with no singleton copy:
`initialize_clone`, `initialize_copy`, `initialize_dup`,
`instance_variables_to_inspect`, `pp`, `respond_to_missing?`.

Mechanically this is `def self.foo` → `module_function def foo` in
`builtins/{kernel,process,objspace,signal}.rs` and wherever `FileTest`'s rows
live. The DSL already supports the keyword (`builtins/math.rs:72`). Watch the
`_recv` binding: under `module_function` it is the module for the singleton call
and the includer for the instance call, so any body that uses `recv` needs
review.

Closes ~170 reflection entries and 3 functional defects.

**Status: done.** Process, Signal and ObjectSpace converted in `20030926`;
FileTest rebuilt as a real 26-row module in `236c21f3`. Process now matches the
oracle exactly (39 private / 47 singleton), FileTest 26/26, Kernel 43 singleton
where it had none. Measured effect on the census: unreachable 255 → 212,
private-unreachable 174 → 122, zeo-only 318 → 279.

### 1.2 Complete `Errno`

zeo has 23 of CRuby's 158 `Errno` constants. `rescue Errno::ERANGE` is a
`NameError` today.

The set is hand-maintained in **three** places that must agree:

- `crates/zeo/src/parse/mod.rs:134` — the Ruby prelude (`class ENOENT <
  SystemCallError`), 12 entries.
- `crates/zeo-abi/src/lib.rs:2159` — the `BUILTINS` rows.
- `crates/zeo-rt/src/builtins/file.rs:22` — `raise_errno`'s `io::Error` →
  class-name match.

Adding 135 rows by hand across three files is exactly the drift the arity work
just finished removing. Generate all three from **one** table keyed on the libc
constant, carrying `(name, errno, strerror-message)`. `raise_errno` then becomes
a lookup on `raw_os_error` instead of a hand-written match, which also fixes the
`SystemCallError`/"Unknown error" fallback for every unmapped code.

Also needs: `Errno::EXXX::Errno` (the integer constant on each class),
`SystemCallError.new(errno)` returning the right subclass, and
`SystemCallError#errno` (currently unreachable). Aliases matter —
`Errno::EWOULDBLOCK` is the *same class* as `Errno::EAGAIN` in CRuby, not a
subclass, which is why the census sees 158 constants but fewer distinct classes.

**Status: done.** The one table is `crates/zeo-abi/src/errno.rs`
(`ERRNO_CLASSES` + `ERRNO_ALIASES`), carrying `(name, errno)` only — the message
comes from `strerror` at run time, as CRuby's does, so 107 message strings never
had to be written down. `EXCEPTION_CLASSES` splices the block in with a const
fn, the compiler registers it in one loop, and `raise_errno` is a lookup on
`raw_os_error`.

The two facts that shaped it:

- **The alias half is bigger than the class half suggests.** 51 of macOS's 158
  names are second spellings, and 50 of those name an errno macOS does not
  define at all, which CRuby binds to `Errno::NOERROR`. They are registered as
  compiler aliases plus runtime *constants*, since a constant is what makes them
  appear in `Errno.constants` — a `builtin_overlay` alone answers `rescue` and
  `const_get` but lists nowhere.
- **The message is composed, not stored.** `SystemCallError#initialize` builds
  `"#{strerror(errno)} - #{msg}"`, but every internal `raise_error` site has
  already composed the whole line CRuby prints
  (`"No such file or directory @ rb_sysopen - /nope"`). `construct_exception`
  therefore puts the message back verbatim for the `SystemCallError` family.
  CRuby splits the same way: `rb_syserr_fail_str` never runs
  `syserr_initialize`.

The block moved to the END of `EXCEPTION_CLASSES` (its length is a platform
fact, so no later id can be a literal), which renumbered every exception id
after `SystemCallError`. `tests/errno_full_surface.rb` matches the oracle
byte for byte. Linux gets a second table derived from `libc`'s `asm-generic`
values; it is not oracle-verified, and the architectures that renumber errno
(mips, sparc, parisc, alpha) would need one of their own.

### 1.3 `object_id` on classes and modules

`Array.object_id == Hash.object_id` today. Derive it from the `ClassId` the way
the other identity paths already do.

**Status: done** in `1d1f752d`. `Kernel#object_id`'s fallback arm takes the
address of the value it was handed, and a `Class` is a bare `ClassId`, so
`recv` points at the caller's temporary slot — two classes asked in a loop read
back the same address. Class ids now sit in their own band above Symbol's.

### 1.4 The census as a ratchet

The measurement lived in throwaway scratch scripts, so nothing stopped the
numbers going back up.

**Status: done.** `tools/method_census.rb` is the walker, and it runs UNCHANGED
under both engines — that is what makes the two dumps comparable.
`cargo run -p xtask -- method-census` records the oracle side into
`conformance/method-census.tsv`; `crates/zeo/tests/method_census.rs` compiles
the same walker through zeo, diffs, and gates every gap against
`conformance/method-census-gaps.tsv` (642 rows). A new gap fails; a closed row
must be deleted, so the file can only shrink. `ZEO_BLESS=1` re-records it. No
ruby is needed at test time, and the whole test runs in 0.2s once built.

Four tags, because the four need different work: `absent-module` 41,
`unreachable` 211, `owner` 183, `constant` 200. `absent-module` deliberately
does NOT also list that module's methods — the module row implies them, and
listing both would double-count the same work.

Methods zeo has and CRuby does not are deliberately not gated. Every exception
class registers the shared `Exception` natives on its own id (flat dispatch), so
that direction is dominated by a design choice rather than by bugs and would
bury the rows that matter.

`tools/method_coverage.rb` is DELETED. It sampled a representative instance for
38 hand-listed classes and counted inherited methods repeatedly — the
methodology behind the wrong "~1,976 missing" figure this plan opens by
correcting.

## Wave 2 — missing methods on classes zeo already has (255)

Ordered by count. Each lands with a golden blessed from the oracle.

| target | n | notes |
|---|---|---|
| `Numeric` instance | 19 | **done**, see 2.1 below |
| `Thread` | 24 | `Thread.start` (a very common idiom), `.kill`, `.exit`, `.stop`, `.abort_on_exception`, `#backtrace`, `#raise`, `#run`, `#wakeup`, `#priority`, `#stop?`, `#fetch` |
| `Module`/`Class` | 12 | `autoload`, `autoload?`, `const_missing`, `const_source_location`, `remove_class_variable`, `set_temporary_name`, `public_instance_method`, `Module.nesting`. `autoload` is load-bearing for real gems |
| `GC` | 12 | `stat_heap`, `config`, `total_time`, `measure_total_time`, `latest_gc_info`, the compaction family. Mostly can report honest constants for a non-MRI heap |
| `Encoding::{InvalidByteSequence,UndefinedConversion}Error` | 12 | `source_encoding`, `destination_encoding`, `error_bytes`, `error_char`, … |
| `Fiber` | 11 | `Fiber.yield` unreachable as a *singleton listing*; `#raise`, `#backtrace`, the scheduler hooks |
| `IO` | 7 | `print`, `puts`, `syswrite`, `ioctl`, `timeout`, `timeout=`, `set_encoding_by_bom` |
| `File`/`File::Stat` | 14 | `File#atime`/`birthtime`/`ctime`, `File.chown`/`lchmod`/`lchown`/`lutime`, `Stat#*_real?`, `dev_major`/`minor` |
| `TracePoint` | 8 | `binding`, `self`, `return_value`, `parameters`, … |
| `Binding` | 4 | `implicit_parameter*`, `irb` |
| `Process::Tms` | 7 | it is a `Struct`; the accessors and `Struct` class methods |
| `Thread::Queue`/`SizedQueue` | 5 | `clear`, `num_waiting`, `marshal_dump` |
| long tail | ~40 | `Marshal.restore` (alias of `load`), `Symbol.all_symbols`, `Regexp.timeout=`, `Random.seed`, `Hash.ruby2_keywords_hash`, `Set#compare_by_identity`, `Warning#warn`, `Warning.categories`, `ThreadGroup#enclose`, `Dir.chroot`, `String#unicode_normalize!`, `Exception#backtrace_locations`, `NameError#local_variables`, `SyntaxError#path`, `Refinement#target`, `Enumerator::Lazy#eager`, … |

### The four scope decisions

Taken before Wave 2.2, because each one changes what "done" means:

1. **Honest stubs for capabilities zeo lacks.** `GC` heap statistics,
   `Thread#priority`, `Fiber`'s scheduler hooks and `TracePoint`'s
   frame-dependent rows all get defined and answer a TRUTHFUL degenerate value
   — `0`, `{}`, `nil`, or the stored value of a setter nothing reads. This is
   the rule `GC.start` already follows. Every one gets a `COMPATIBILITY.md`
   line. `TracePoint#self`/`#binding`/`#return_value` are the exception: they
   raise CRuby's own `RuntimeError` for an event that cannot supply them,
   because a fabricated `self` is worse than a loud refusal.
2. **All 103 encodings registered; the single-byte ones implemented for real.**
   See Wave 3.6.
3. **`Ractor`: error classes only.** The eight classes so `rescue
   Ractor::ClosedError` resolves; the 23 methods stay deferred with the reason
   already written above.
4. **`Pathname` vendored and default-loaded**, matching the oracle with no
   `require`. See Wave 3.5.

### 2.1 `Numeric` instance-method defaults (19)

**Status: done.** All 19 rows are written through `send`, so a subclass that
defines only `<=>`, `-`, `/`, `*`, `to_f`, `to_i`, `to_r` and `coerce` inherits
the whole family. `tests/numeric_subclass_defaults.rb` matches the oracle byte
for byte.

Two things had to be fixed underneath it:

- **zeo had no builtin `undef`.** `Complex(1, 2).positive?` answered `false`
  where CRuby raises, along with `divmod`, `step`, `clamp`, `between?`,
  `remainder` and `negative?`. `mark_undefined` existed and `respond_to?`
  honoured it, but `flat_value_hit` flattened every ancestor's methods without
  consulting `undefined_methods`, so the call still landed. Fixing it was a
  PREREQUISITE: the generic `Numeric` rows would otherwise have widened the
  leak by eight more names.
- **`Kernel#Float` refused a `Numeric` subclass**, which broke `#floor`,
  `#round` and `#coerce` on exactly the classes this wave exists for. It now
  uses the `to_f` conversion protocol.

Two limits are recorded as XFAIL rather than shipped quietly
(`tests/gaps/numeric_subclass_limits.rb`): there is no
`singleton_method_added` hook, so `def n.foo` on a Numeric still succeeds; and
`Numeric#i` cannot build a Complex whose component is a subclass, because every
`cpx_*` routine assumes a native lane.

### 2.2 `Thread` (24)

`builtins/thread.rs` is a HAND-ROLLED `match`-on-`&str` table, not the DSL, and
it has the exact drift the arity work removed everywhere else: `lookup_names`
lists `abort_on_exception` twice and `lookup_class_names` omits it entirely, so
two of the 24 "missing" rows are a stale listing rather than a missing method.
Every row also reports arity `-1` from a hand-written `lookup_arity`.

So this wave MIGRATES `Thread` to `ruby_class!` first, then adds rows. The
migration is what makes the additions cheap, and it puts `Thread` into the
compiler's `CLASS_SURFACE` for the first time. `Fiber` gets the same treatment
in Wave 2.5.

The rows, by what they need:

- **Free from existing runtime state**: `Thread.start`/`.fork` (aliases of
  `.new`), `.kill`/`.exit` (the class forms of `#kill`), `#fetch` (over
  `thread_local_get`), `#pending_interrupt?` and `.pending_interrupt?` (over
  `interrupt_pending`), `#run`/`#wakeup` (over `wake_target`).
- **New process- or thread-wide cells**: `.ignore_deadlock`/`=`,
  `#priority`/`#priority=` (stored, advisory — CRuby's is advisory too on most
  platforms), `#native_thread_id`.
- **Honest stubs**: `Thread.stop` sleeps until woken; `#backtrace` and
  `#backtrace_locations` answer the CURRENT thread's real frames and `nil` for
  a dead thread, matching CRuby, but `[]` for another live thread, which
  `crate::frames` cannot reach across threads. `#set_trace_func`/
  `#add_trace_func` accept `nil` and refuse a Proc.
- `Thread.each_caller_location` over the same frame walk as `#backtrace`.

**Status: done.** All 24 rows closed (`unreachable` 192 → 169, `owner` 183 →
182), and the migration paid for itself twice over: the 57 rows now declare
their shape, and `conformance/builtin-arity.tsv` — regenerated so it covers
`Thread` for the first time — accepts every one with NO divergence row. The
`lookup_names` drift is structurally gone, since one parameter list now feeds
the guard, the arity and the listing. `tests/thread_surface.rb` matches the
oracle byte for byte; the four narrowings are in `docs/COMPATIBILITY.md`.

### 2.3 `Module` reflection (12)

`autoload` is the interesting one, and it is nearly free: zeo resolves
`autoload :C, "feature"` at COMPILE time by eagerly splicing the feature
(`parse/loader.rs`), so by the time any program runs, every autoloaded constant
is already defined. `Module#autoload` therefore lowers to a no-op that answers
`nil`, and `#autoload?` answers `nil` — which is what CRuby answers for a
constant that has ALREADY loaded. The observable behaviour matches; only the
timing differs, and that divergence is already documented.

The rest: `const_source_location` (the compiler knows the defining file and
line and must thread it into the constant store), `const_missing` (the default
raises `NameError`; the hook itself needs the constant-miss path to `send` it),
`public_instance_method`, `remove_class_variable`, `set_temporary_name`,
`Module.nesting`, `refinements`/`used_modules`/`used_refinements` (zeo already
mints a `Refinement` per `refine` block, so these are listings over data that
exists), and `undefined_instance_methods` — which reads the
`undefined_methods` set Wave 2.1 just made load-bearing.

**Status: done**, `unreachable` 169 → 155. Three findings shaped it:

- **`Module.nesting` cannot be answered by a runtime row at all.** Nesting is
  the LEXICAL chain at the call site, and a builtin row has no view of its
  caller's scope. Codegen already tracks that chain (`Ctx::cref_chain`), so
  the literal call folds to the array and the runtime row answers `[]` — which
  is what CRuby answers at top level anyway.
- **`autoload?` can be faithful without loading anything.** The literal
  `autoload` form never reaches the runtime row (it is spliced and lowered to
  a no-op), so the row only ever sees the residue the collector cannot find.
  Recording its path there makes `autoload?` answer exactly what CRuby answers
  in BOTH cases: the path for a feature that has not loaded, `nil` once the
  constant resolves.
- **`Complex.undefined_instance_methods` matches the oracle name for name** —
  the same 19 the Wave 2.1 undef list installs, which is an independent
  confirmation that that list is right.

`Kernel.autoload`/`autoload?` moved from `unreachable` to `owner`: they answer
now, through `Module`, but CRuby files them on `Kernel` too. That is Wave 4's.

### 2.4 `IO`, `File`, `File::Stat` (21)

The most mechanical wave. `IO#print`/`#puts` already exist as `Kernel`
intrinsics and need the `IO`-receiver rows that write to THAT stream;
`#syswrite`, `#ioctl`, `#timeout`/`#timeout=` and `#set_encoding_by_bom` are
thin. `File#atime`/`#birthtime`/`#ctime` delegate to the `File::Stat` rows that
already exist. `File.chown`/`.lchmod`/`.lchown`/`.lutime` are libc calls beside
the ones `file.rs` already makes. `Stat#dev_major`/`#dev_minor`/`#rdev_major`/
`#rdev_minor` are bit arithmetic on fields `stat.rs` already holds, and the
`*_real?` trio is `access(2)` with the real rather than effective uid.

**Status: done**, `unreachable` 155 → 132 — 23 rows for 21 named, because two
of them were never missing at all.

`IO#puts` and `IO#print` HAD table rows the whole time. They were dropped by
`dispatch::is_hidden_builtin_private`, a name-only list that hid `puts`,
`print`, `raise`, `warn` and friends from reflection **on every class**,
because they are private on `Kernel`. So `IO#puts`, `StringIO#print`,
`Thread#raise` and `Fiber#raise` all vanished from their own listings. The list
is now keyed on the OWNER, and the names it does cover are CLASSIFIED private
rather than skipped — `Kernel.private_instance_methods(false)` was missing them
too, since dropping a name removes it from both halves at once.

`File.open(path, "rb")` did not set binmode, so `#binmode?` was wrong and
`#set_encoding_by_bom` had nothing to gate on. The `b` in a mode string now
sets it.

### 2.5 `GC`, `Fiber`, `Binding`, `TracePoint` (35)

Decision 1 governs this wave. `GC` gets the 13 rows as honest stubs plus its
three constants (`OPTS`, `INTERNAL_CONSTANTS`, `Profiler`), reporting what is
TRUE of a refcounted heap — zero collections, no compaction, an empty
`stat_heap`. `GC.config` answers the two keys CRuby answers.

`Fiber` migrates to `ruby_class!` alongside `Thread`, then gains `#raise` on the
listing (it is already implemented, just absent from `lookup_names`),
`#backtrace`, `#blocking?`/`.blocking`/`.blocking?`, and the scheduler quartet
(`.scheduler`, `.set_scheduler`, `.current_scheduler`, `.schedule`) — zeo has
no fiber scheduler, so `.scheduler` answers `nil` and `.set_scheduler(nil)`
succeeds while a non-nil scheduler raises rather than being silently ignored.
`Fiber.yield` is implemented and only missing from the singleton listing.

`Binding`'s four are small: `#irb` raises the same `LoadError`-shaped refusal
CRuby gives without the gem, and the `implicit_parameter*` trio reports on `it`
and the numbered block parameters the compiler already tracks.

`TracePoint` gets `#parameters`, `#eval_script`, `#instruction_sequence`,
`.allow_reentry` and `.stat`; `#self`, `#binding` and `#return_value` are
defined and raise, per decision 1.

**Status: done**, `unreachable` 132 → 98, `constant` 198 → 196 and `owner`
184 → 182. Three things are worth recording:

- **`GC.latest_gc_info` and `.latest_compact_info` match the oracle exactly.**
  The shape CRuby answers in a process that has not yet collected is the shape
  a refcounted heap answers forever, so those two are not stubs at all. The
  counters (`count`, `total_time`) had to become type assertions in the golden
  anyway: CRuby's own values move between runs.
- **`Fiber` migrated to `ruby_class!` alongside `Thread`**, which is what
  freed `#inspect`/`#to_s` — they had no rows at all and fell back to
  `Object`'s. `Fiber#backtrace` answers `[]` rather than `nil` for a
  terminated fiber, which is CRuby's answer and the opposite of `Thread`'s.
- **The golden found a 30-second latency bug.** `Thread.kill(t)` on a thread
  that had not yet reached its `sleep` posted the interrupt before
  `sleep_impl` registered a ctx to wake, and the loop checked interrupts only
  AFTER parking — so the kill took effect when the sleep expired. Both sleep
  loops now check before parking. `tests/thread_surface.rb` went from 30.0s to
  0.0s.

`TracePoint#self` and `#binding` are the one place a listed row refuses where
CRuby answers, so they are XFAIL in `tests/gaps/tracepoint_self_and_binding.rb`
with the fix shape written down.

### 2.6 Encoding error classes and the long tail (~55)

`Encoding::InvalidByteSequenceError` (7) and `Encoding::UndefinedConversionError`
(5) need the transcoder to ATTACH its context to the exception rather than
formatting it all into the message. `enc/transcode.rs` already knows the source
encoding, destination encoding and offending bytes at the raise site;
`transcode_signal` currently discards them.

The long tail is ~40 independent one-liners, each with its own oracle-blessed
assertion in one golden. Four of them close a whole absent module by
themselves: `Object::Mutex`/`Queue`/`SizedQueue`/`ConditionVariable` are
top-level constants CRuby aliases onto the `Thread::*` classes, and zeo
registers only the nested spelling.

**Status: done**, `unreachable` 98 → 41, `owner` 182 → 156, `constant` 196 →
190. The 41 that remain are exactly the two deferred sets: `Ractor` (23) and
`Kernel`'s compiler intrinsics (18).

The wave paid for itself three times over, because closing rows meant fixing
what was underneath them:

- **Wave 4's enumeration bucket closed here, in nine lines.** Every exception
  class registers all twelve `Exception` natives on its own id (flat dispatch),
  but `define_method_own` never touched `own_methods`, which is what the
  listing walks — so `Exception.instance_methods(false)` was empty. Marking
  them is not enough on its own: it has to happen only on the class CRuby OWNS
  each one on, or `MyError.instance_methods(false)` would report Exception's
  twelve where CRuby reports none. One `mark_owned_names` table does it, and
  `owner` fell by 26 rows at once.
- **UTF-8 never reported an incomplete sequence.** `decode_utf8` used
  `InvalidStyle::Plain` for every failure, so `"\xC2".encode(...)` said
  `"\xC2" on UTF-8` where CRuby says `incomplete "\xC2" on UTF-8`. The
  multibyte decoders already drew the distinction; `std::str::from_utf8`'s
  `error_len() == None` is exactly it. That fixed the message AND gave
  `#incomplete_input?` something true to report.
- **`Process::Tms` was Float-only.** CRuby's is a `Struct`, so
  `Process::Tms[1, 2, 3, 4]` keeps Integers. The four slots now hold arbitrary
  values, which is what let the writers and `Struct.[]`/`.members`/
  `.keyword_init?` land at all.

Three rows read the oracle rather than the docs, and are worth recording:
`Warning#warn` is a PUBLIC instance method (warning.c defines it on the module
and then extends the module with itself), not a `module_function`;
`Mutex#sleep` answers `nil`, not the elapsed seconds; and `Encoding._load`
answers what it was handed rather than the encoding.

`NoMatchingPatternKeyError`'s accessors match the oracle, but zeo's pattern
matcher still raises the PARENT class — `codegen/patterns.rs` compiles a whole
pattern to one boolean, so the raise arm cannot name the failing key. XFAIL in
`tests/gaps/pattern_key_error_class.rb` with the fix shape.

## Wave 3 — classes zeo lacks (335, minus the out-of-scope 161)

| class | n | why |
|---|---|---|
| `Pathname` | 110 | CRuby loads it by default in 4.0 even under `--disable-gems`. Almost pure Ruby — vendor `lib/pathname.rb` the way the other gems are vendored, and back the few native bits with existing `File` methods |
| `Process::Sys` / `UID` / `GID` | 68 | thin libc wrappers over `set*id`; `Process` already links libc |
| `Encoding::Converter` | 17 | the encoding engine exists; this is its public face |
| `Enumerator::ArithmeticSequence` | 13 | **observable today**: `(1..10).step(2).class` is `Enumerator` in zeo, `Enumerator::ArithmeticSequence` in CRuby, and `Array#[]` accepts one |
| `GC::Profiler` | 8 | can be honest no-ops that report zero |
| `Thread::Backtrace` + `::Location` | 8 | `crates/zeo-rt/src/builtins/backtrace_location.rs` already exists under a different constant path — mostly a re-parenting |
| `ObjectSpace::WeakKeyMap` | 7 | `WeakMap` already exists; sibling shape |
| `Random::Base` / `Random::Formatter` | 6 | `Formatter` is the `SecureRandom` surface |
| `Enumerator::Generator` / `Producer` | 4 | backs `Enumerator.new` and `Enumerator.produce` |
| `Ractor::*` error classes | 8 | decision 3 — the classes only, so `rescue` resolves |

Every new class needs a `zeo_abi::BUILTINS` row. **`BUILTINS` is indexed by
id**: append, never insert, and a nested name needs its lexical parent at an
earlier index. The highest id in use today is 148.

### 3.1 `Enumerator::ArithmeticSequence` (13)

**Status: done** — `absent-module` 41 → 40, `constant` 190 → 189, and no new
row of any tag. `tests/arithmetic_sequence_rows.rb` is byte-identical to the
oracle across 100 lines.

It is an `EnumSource` variant, not a new value type: an arithmetic sequence
IS an Enumerator that also carries `(begin, end, step, exclude_end)`, and
`enumerator_class_id` reports the subclass off that variant, exactly as
`Enumerator::Chain` and `::Product` already did. So every inherited method
kept working for free, and the thirteen own rows are the ones the quadruple
makes possible.

Three things it forced open, each a real fix rather than scope creep:

- **One walk, shared.** `Range#step`, `Range#%`, `Numeric#step` and
  `ArithmeticSequence#each` now all go through `numeric::step_walk`. They had
  to: a sequence's `to_a` must equal the block form that would have built it.
  Unifying them fixed `Numeric#step`'s Float lane, which summed repeatedly and
  drifted — `0.0.step(1.0, 0.1).to_a` ended on `0.9999999999999999` where
  CRuby ends on `1.0`. `Range#step` already computed `begin + i * step`; the
  shared walk is that one.
- **`Range#step` handled only Integer endpoints** and panicked on anything
  else, so an endless or Rational range could not be stepped at all. The
  generic walk removes the panic for every numeric range. A non-numeric range
  (`("a".."e").step(2)`) still answers the right blockless Enumerator and
  still panics when walked — unchanged, and not this wave's business.
- **`Range#step` and `Range#%` had to split.** They shared a `|`-joined def,
  and `__RUBY_METHOD` binds the PRIMARY name, so `%` could not have printed
  `((1..10).%(2))`. Splitting them also separated their arities, which the
  oracle says differ: `step` is `-1` (C discarded the signature), `%` is `1`.

`Array#[]` reads a sequence through `rb_arithmetic_sequence_beg_len_step`'s
rules, which are stricter than the equivalent Range's and were derived from a
44-case oracle matrix rather than the C: an out-of-range span RAISES where
`a[20..30]` answers nil, a negative step swaps the endpoints and walks the
span backwards, and **a step of exactly 1 routes through the ordinary Range
slice instead** — which is why `a[(11..12).step(1)]` is nil while
`a[(11..12).step(2)]` raises.

### 3.2 `Process::Sys` / `Process::UID` / `Process::GID` (68)

**Status: done** — `absent-module` 40 → 37, `constant` 189 → 186, no new row.
`tests/process_identity_rows.rb` is byte-identical to the oracle, and nothing
in it can change an id: an unprivileged process may only re-assert the ids it
already holds, so every reader, every re-assertion and every refusal is
exercised without needing (or being able to gain) privilege.

`Process::UID` and `Process::GID` are the same ten names over two families of
syscalls, so both modules' rows call one set of helpers with an `IdKind`
saying which. That is the whole reason the pair costs almost nothing beyond
the one.

Two details the oracle settled, and neither was guessable:

- **The id conversion is `rb_num2ulong`, not `rb_num2long`.** `nil` therefore
  takes the ordinary `to_int` refusal (`no implicit conversion of nil into
  Integer`), NOT the lowercase `from nil to integer` that every other
  integer-argument site in zeo raises, and a bignum names `'unsigned long'`.
  A String is a passwd or group NAME first, and only a failed lookup is an
  error (`can't find user for x`).
- **The failure message is bare.** `rb_sys_fail(0)` raises `Errno::EPERM`
  with `Operation not permitted` and nothing appended — no ` @ syscall -
  path` suffix, which is what `file.rs`'s `raise_errno` would have produced.

One deliberate narrowing: `Process::Sys.setresuid`/`setresgid` are the
not-implemented stubs on every platform. CRuby installs those wherever the
syscall is absent, and the stub reports **arity 0** while accepting any
arguments — so the real three-argument form cannot share a declaration with
it, and the arity ledger is single-platform. Recorded in
`docs/COMPATIBILITY.md`; `Process::Sys.setreuid` and
`Process::UID.change_privilege` reach the same capability by another name.

### 3.3 The small absent classes (33, plus the 6 Ractor errors)

**Status: done** — `absent-module` 37 → 23, `constant` 186 → 173, no new row.
`tests/small_core_classes.rb` is byte-identical to the oracle.

Landed: `GC::Profiler` (8), `Thread::Backtrace` (1), `ObjectSpace::WeakKeyMap`
(7), `Random::Base` (3) + `Random::Formatter` (2), `Enumerator::Generator` (1)
+ `Enumerator::Producer` (1), and the six `Ractor` error classes.

Three of these were structural rather than additive:

- **`Thread::Backtrace::Location` was already there and still unreachable.**
  The compiler seeds nested builtin names by looking the lexical parent up
  among the classes seeded SO FAR, and `Thread::Backtrace` has to take a
  higher id than the location it namespaces (ids are append-only). The fix is
  a second pass that resolves every nested name against the ABI names
  directly, so a parent declared later still binds — one class closed two
  census rows.
- **`Random`'s ancestry was two rungs short.** `#rand`/`#bytes`/`#seed` are
  `Random::Base`'s methods in CRuby, not `Random`'s, and `Random::Formatter`
  mixes in BELOW them — which is exactly what lets `Random#rand` win over the
  formatter's own `rand` while `#random_number` still resolves. Moving the
  three rows down a rung is a Wave 4 owner fix taken early, because the class
  could not be added without it.
- **`Enumerator::Generator`/`Producer` are the enumerator's SOURCE**, not
  enumerators. `Enumerator.new { |y| }` now holds a real Generator and
  delegates `each` to it, which is CRuby's own shape — and it means both new
  `each` rows are load-bearing rather than decorative, and
  `#<Enumerator: #<Enumerator::Generator:0x…>:each>` falls out of the ordinary
  method-source rendering instead of being special-cased.

One accepted **widening**: `Random::Formatter` had to be ungated (CRuby has it
in core), and zeo gates whole classes rather than methods — so the nine
`hex`/`uuid`/`base64` methods `require "random/formatter"` adds in CRuby are
present from the start. Nine `zeo-only` rows in
`conformance/builtin-arity-divergences.tsv`, whose header now says why. zeo
answers where CRuby raises `NoMethodError`, never the reverse.

The Ractor classes are classes only, per decision 3 — nothing here is ever
raised. `Ractor::ClosedError` descends from `StopIteration`, not from
`Ractor::Error`, which is what lets `Kernel#loop` swallow it;
`Ractor::RemoteError#ractor` is the tree's one method and answers nil.

### 3.5 `Pathname` (96 own methods)

Vendored under `gems/pathname/`, the way the other 40 gems are, and
DEFAULT-LOADED so it matches the oracle with no `require` — CRuby 4.0 has it
reachable under `--disable-gems`. CRuby splits it between `pathname.rb` and a C
extension; the C half is small (`#initialize`, `#==`, `#<=>`, `#hash`, `#to_s`,
`#sub`, `#sub_ext`, and the `File`/`Dir` delegators) and every one of those has
a `File` or `Dir` row already. `Kernel#Pathname` comes with it.

### 3.6 The encoding registry — all 103, single-byte implemented

Decision 2, and the largest constant bucket by far: 162 of the 198 missing
constants are encodings. Two parts, one commit each:

1. **Register all 103.** A table generated from the oracle carrying each
   encoding's canonical name, aliases, `dummy?` and `ascii_compatible?`. That
   alone makes `Encoding.list`, `Encoding.name_list`, `Encoding.find`, every
   constant, and every reflection answer match CRuby exactly, because those
   four facts are ALL reflection reads. An operation that would need a mapping
   zeo does not have raises rather than transcoding wrongly.
2. **Implement the ~40 single-byte families for real** — the `CP*`, `IBM*`,
   `ISO-8859-*` and `KOI8-*` rows. These are pure 256-entry data tables and
   drop straight into the existing `EncKind::SingleByte` machinery, so they
   cost data rather than code. The multibyte families (EUC-TW, GB18030, the
   Big5 variants, the ISO-2022 family) stay registered-only.

The split matters: part 1 closes all 162 census rows on its own. Part 2 closes
no census rows at all — it converts registered-only encodings into working
ones, which the census cannot see and a golden must.

**Status: done**, both parts in one commit — `constant` 173 → 12, and the only
`Encoding` constant left is `Converter` (3.4). The two parts came from ONE
generator pass (`tools/encoding_tables.rb` writes `enc/registry.rs` and
`enc/single_byte.rs` together), so splitting them would have meant generating
the same tables twice. Both review artifacts are present: the census diff for
part 1, `tests/encoding_registry.rb` for part 2.

Taken **before 3.4** rather than after, because `Encoding::Converter`
needs it: `.asciicompat_encoding("ISO-2022-JP")` answers
`stateless-ISO-2022-JP` and every `#convpath` through ISO-2022-JP pivots
through it, so the class could not answer correctly over a 27-row registry.
Doing 3.6 first also meant the converter's edge table was generated once,
against the final registry.

What the oracle settled, none of it guessable:

- **The row order, the aliases and the constant spellings are all data**, so
  they are generated rather than written. CRuby gives one name up to TWO
  constants — the name with every non-alphanumeric character turned into `_`
  and a leading lowercase letter capitalized (`eucJP` → `EucJP`), plus that
  spelling fully upcased whenever the name carries a lowercase letter
  (`WINDOWS_1250` beside `Windows_1250`) — except that a name with no
  uppercase letter gets only the upcased form (`ebcdic-cp-us` → just
  `EBCDIC_CP_US`) and a name opening with a digit (`646`) gets none. The rule
  was derived by testing candidates against all 174 names, not read off the C.
- **`locale`/`external`/`filesystem`/`internal` are not table data.** CRuby
  moves them onto whichever row `Encoding.default_external`/`.default_internal`
  currently names, so `#names` computes them. `internal` is a registered alias
  SLOT: while `default_internal` is unset it belongs to no row, and
  `Encoding.name_list` still lists it — the one name that is in `name_list` and
  in no row's `#names`.
- **`Latin-1` is not one of CRuby's spellings.** zeo carried it as an
  ISO-8859-1 alias; it resolved a name CRuby refuses, and it is gone.
- **Four single-byte rows have no transcoder in CRuby either.**
  `Windows-1258`, `GB1988`, `macCentEuro` and `macThai` are structurally
  single-byte, and every one of their high bytes is a valid character with no
  Unicode mapping to be had. A generated table of nothing would have said
  "undefined conversion" where CRuby says "converter not found", so the
  generator leaves those rows registered-only.

One adjacent fix the registry golden forced open: **`Encoding.compatible?`
asked its two sides in the wrong order.** Two ascii-only strings take the
FIRST one's encoding, not the second's, and an empty first side keeps its own
encoding only when that encoding could have carried the other side's text as
it stands. Six of the 100 pairs in `tests/encoding_compatible.rb` were wrong;
all 100 match now.

## Wave 4 — owner and enumeration fidelity (157 + 25)

Split by probe, not assumed:

- **157 owner divergences.** `Array#map`'s owner is `Enumerable` (21 such on
  `Array` alone), `Float#<`'s is `Comparable`, `Array#inspect`/`#hash`/`#to_s`'s
  is `Kernel`. These work, so the cost is `.owner`, `instance_method`, `super`
  from a subclass override, and the error-message text CRuby produces from the
  specialized version. Fix by moving the row to the class CRuby owns it on.
- **25 enumeration-only.** `Exception.instance_methods(false)` returns `[]` in
  zeo while `Exception.instance_method(:message).owner` correctly says
  `Exception` — the table is right and the listing does not walk it.

Wave 4 is last because it changes no behavior, and because moving rows is
cheapest once Waves 1–3 have stopped adding new ones.

## Verification

- Per commit: `cargo build -p zeo-rt` plus the affected golden. Cheapest
  sufficient check; full suite at wave boundaries only; tee once and slice the
  log rather than re-running for detail.
- The census **is** the ledger, and it now runs as a test — see 1.4. Every wave
  ends with `ZEO_BLESS=1 cargo test -p zeo --test method_census`, whose diff is
  the review artifact: the rows it deletes are exactly what the wave closed.
- Goldens blessed with `ZEO_BLESS=1` against `mise exec ruby@4.0.6`.
- Wave boundaries: `cargo nextest run --workspace`, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`, `cargo fmt --all`.
- `cargo run -p xtask -- bench --runs 5` after Wave 1 and Wave 4 — those are the
  two that touch dispatch tables. Never `bench --filter X --update-baseline`.

## Files

- `crates/zeo-rt/src/builtins/{kernel,process,objspace,signal}.rs` — Wave 1.1
  (`FileTest` has no file of its own yet).
- `crates/zeo-abi/src/errno.rs` — Wave 1.2, the one table behind
  `crates/zeo/src/parse/mod.rs`, `crates/zeo-abi/src/lib.rs` and
  `crates/zeo-rt/src/builtins/file.rs`.
- `tools/method_census.rb`, `crates/xtask/src/method_census.rs`,
  `crates/zeo/tests/method_census.rs`, `conformance/method-census*.tsv` —
  Wave 1.4, the ratchet.
- `crates/zeo-rt/src/builtins/{numeric,thread,rmodule,gc,io,file,stat,fiber,encoding}.rs`
  — Wave 2.
- New files under `crates/zeo-rt/src/builtins/` per class, plus `zeo-abi`
  `BUILTINS` rows — Wave 3. **`BUILTINS` is indexed by id**: append, never
  insert, and a nested name needs its lexical parent at an earlier index.
- `gems/pathname/` — Wave 3.5, vendored and default-loaded.
- `crates/zeo-rt/src/enc/table.rs`, `crates/xtask/src/encoding_table.rs` —
  Wave 3.6, the 103-row registry generated from the oracle.
