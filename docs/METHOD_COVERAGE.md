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
| `Numeric` instance | 19 | `%`, `+@`, `-@`, `abs`, `ceil`, `coerce`, `div`, `divmod`, `floor`, `round`, `truncate`, `to_int`, `finite?`, `infinite?`, `numerator`, `denominator`, `magnitude`, `modulo`, `i`. **Not cosmetic**: these are the defaults a user's `class MyNum < Numeric` inherits. Integer/Float already answer their own |
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
- `gems/pathname/` — Wave 3, vendored.
