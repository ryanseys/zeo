# Bundler north star — compile-`bundle` inventory

**Goal (plan Phase 6 §G):** zeo compiles `rubygems` + `bundler` out of the box,
milestones `bundle --version` → `bundle install --local` → network install.

**Status (2026-07-23): does NOT compile yet, but the architecture is now in
place.** The dominant architectural blockers — `rbconfig`, non-top-level
`require`, and now `securerandom` (with `Random::Formatter` + OS-entropy
`Random.urandom` + `extend`-onto-class/module) — are fixed, as are the dynamic
`load`/`require`, `a, = rhs` multi-assign, and `rescue *splat` compiler gaps,
and native **`etc`** is DONE. The require graph now clears config_file.rb and
stops at a MISSING STDLIB (`fileutils`). What remains is the grind: vendor the
pure-Ruby stdlib (`fileutils`/`pathname`/... — see the worklist) and implement
native `io/wait`, one at a time.

## Setup

Upstream source is cloned (gitignored) into `vendor/rubygems` from
`github.com/rubygems/rubygems` (bundler 4.1.0.dev — matches the ruby 4.0.5 ref
tree). Both halves live there:

- rubygems: `vendor/rubygems/lib` (`require "rubygems"`)
- bundler:  `vendor/rubygems/bundler/lib` (`require "bundler"`)
- exe:      `vendor/rubygems/bundler/exe/bundle`

Probe command:

```
zeo -S -I vendor/rubygems/lib -I vendor/rubygems/bundler/lib \
    -e 'require "rubygems"; puts "ok"'
```

`-S` stops after codegen (no rust build) — fastest way to surface the next
lowering gap. zeo is whole-program AOT: it must statically resolve and lower the
ENTIRE reachable require graph, so it stops at the FIRST unsupported construct.

Scale: rubygems + bundler are **~58,500 LOC across 333 `.rb` files**.

## Blockers, in the order zeo hits them

### 1. `require "rbconfig"` — ✅ FIXED
Real Ruby generates `rbconfig.rb` at build time. zeo now ships a synthetic
built-in shim (`crates/zeo/src/parse/shims/rbconfig.rb`, spliced by
`parse/loader.rs::splice_synthetic_shim`) with the keys rubygems+bundler read.
First-pass limitation: static arm64-macOS platform/paths, not derived from the
build target — make it compile-time-generated for cross-target / gem-install
fidelity.

### 2. `expr::CONST` — dynamic-scope constant path — compiler gap (small)
`@rbconfig::CONFIG[key]` (`rubygems/target_rbconfig.rb:48`) — a `ConstantPathNode`
whose scope is a runtime expression, not a constant. `constant_path_name`
(`zeo-hir/src/lower/consts.rs`) rejects a non-constant parent. **Only 1 site**
across all of rubygems+bundler. Fix: a `HirNode::ScopedConstRead { scope: NodeId,
name }` — evaluate the scope, take its `class_id`, runtime `const_get`. Touches
the ~10 HirNode exhaustive-match sites. (Probed past via `@rbconfig.const_get(:CONFIG)`.)

### 3. Non-top-level `require` — architectural — FIXED (was ~252 sites)
`require "securerandom" unless defined?(SecureRandom)`
(`rubygems/util/atomic_file_writer.rb:15`) and ~252 more: a `require` inside a
method / conditional / module / `begin`. The loader's per-file pre-pass
(`RequireCollector` over prism's generic `Visit`) now eager-splices each literal
target, placed BEFORE the file's own statements so a method that
requires-and-uses one works even when called during that file's own load; the
`require` CALL folds to a bool no-op (`lower_call_general`). The
`begin/require/rescue LoadError` optional-dependency idiom compiles now too (for
available features). Over-approximation (a require in a never-taken branch still
loads) is benign under static linking. Not modeled: re-require-returns-false for
nested requires. `load` and non-literal targets stay rejections.

### 4. `autoload` — ALREADY HANDLED (~102 sites, no work needed)
The loader already eager-splices `autoload :C, path` targets (a pre-pass that
mirrors #3) and folds the `autoload` call to nil. Verified during this probe.

## `securerandom` — ✅ FIXED (2026-07-23)

Landed as a faithful, DRY port of the stdlib shape rather than a monolithic
native module:

1. **`Random.urandom` now draws real OS entropy** (`getrandom(2)` /
   `SecRandomCopyBytes`, Gvl-released) instead of the old clock-seeded xorshift
   — a security AND fidelity fix (`builtins/random.rs::os_urandom`). SecureRandom
   sits on this leaf.
2. **Native `Random::Formatter`** (`builtins/formatter.rs`, ABI id 85,
   `require`-gated on `"random/formatter"`) — the mixin providing `hex`/`base64`/
   `urlsafe_base64`/`uuid`/`random_number`/`random_bytes`/`alphanumeric`. Like
   `Comparable` drives `<=>`, every method re-dispatches its entropy leaf
   (`gen_random`) to the RECEIVER via `send_value`, so ONE impl serves
   `SecureRandom`, rubygems' vendored `Gem::SecureRandom`, and a `Random`
   instance. Native (value-receiver) is REQUIRED: only value-receiver bodies can
   run with a Class `self` and redispatch to it (see the extend note below).
3. **`securerandom` shim** (`parse/shims/securerandom.rb`, wired into
   `synthetic_shim_source`) — the thin stdlib wrapper: defines the `gen_random`/
   `bytes` leaf over `Random.urandom` and `extend`s the native formatter.
4. **`extend` now works on Class/Module receivers** (`runtime_meta::
   runtime_extend`, was "can't extend this value"). A native module's methods
   install as the receiver's class methods and run with the Class as `self`
   (redispatch works); a user module's install too (self-contained methods work;
   a user method that redispatches to the host's `def self.x` is the one
   documented limit — zeo represents modules as class values, not objects). Own
   `def self.x` outranks the mixin (`dispatch::class_defines_own_class_method`).

Verified vs oracle: full API shapes match; 3 unit + 11 e2e tests +
`examples/securerandom.rb` + `examples/extend_class_and_module.rb`.

## The grind, in the order zeo hits it (post-securerandom)

**Dynamic `load`/`require` — ✅ FIXED (2026-07-23).** `load ENV["BUNDLE_BIN_PATH"]
if ...` (`rubygems.rb:293`) and any `require`/`load` with a runtime-computed
target no longer fail the compile. Whole-program AOT can't splice a path it only
learns at runtime, so the call now lowers to the runtime `Kernel#{require,
require_relative,load}` (`builtins/kernel.rs`), which raises CRuby's `LoadError`
("cannot load such file -- <path>") if and when it actually executes. A guarded
dynamic load short-circuits at runtime; the `begin; require dyn; rescue
LoadError` idiom works. `lower_call_general` and `Loader::lower_require_statement`
(now `Option`-returning) fall through instead of erroring; literal requires still
splice at compile time. Zero conformance regressions (2335/2335). e2e +
oracle-verified messages.

**Multi-assign trailing comma — ✅ FIXED (2026-07-23).** `spec_tuples, = expr`
(`rubygems.rb:940`), `loc, = caller_locations` (`forwardable.rb:213`), and every
`a, = rhs` / `a, b, = rhs` form: prism models the bare trailing comma as an
`ImplicitRestNode` (an anonymous rest that takes the named lefts and discards
the tail), which `lower_multi_target_group` (`zeo-hir/src/lower/assign.rs`) now
maps to the same anonymous-splat `Some(None)` a written `a, *_ = rhs` uses. 42
zeo-vs-oracle probes (trailing comma × nested/ivar/index/splat-rhs/return
contexts) all pass; e2e added; zero conformance regressions.

**Splat-rescue (`rescue *errs => e`) — ✅ FIXED (2026-07-23).** A `rescue`
clause whose exception list is a splatted runtime array of classes (or a single
class), e.g. `rescue *yaml_errors => e` (`rubygems.rb:410`). `RescueClause` gained
a `splats: Vec<NodeId>` field; the lowering partitions the exception list into
static constants (compile-time `is_a`, fast path unchanged) and splats (runtime
`zeo_rt::rescue_matches_any`), OR'd together. Composes with static classes,
multiple clauses, ensure, retry, `$!`, and module-include matching. 22
zeo-vs-oracle probes pass; e2e added; zero conformance regressions. Documented
edge: a non-Module element (`rescue *[42]`) raises the right `TypeError` but
attributes it to the raise line without the cause-chain (a pathological input).

**Native `Etc` — ✅ FIXED (2026-07-23).** A full native `ext/etc` module
(`crates/zeo-rt/src/ext/etc.rs`, ABI ids 86/87/88, `require`-gated on `"etc"`,
cargo `ext-etc`). Implements the WHOLE surface over libc, not just bundler's
needs: `getpwnam`/`getpwuid`/`getgrnam`/`getgrgid` + the `getpwent`/`getgrent`
cursors and `passwd`/`group` block iterators; `Etc::Passwd`/`Etc::Group` value
structs (all portable members + the BSD `change`/`uclass`/`expire` on Darwin)
with the Struct-like `members`/`to_a`/`to_h`/`each`/`[]`; `sysconf`/`confstr`/
`uname`/`nprocessors`/`getlogin`/`sysconfdir`/`systmpdir`; and the portable
`SC_*`/`PC_*`/`CS_*` constant set libc exposes. 5 unit + 7 e2e tests +
`examples/etc.rb`, oracle-matched. Doc divergence: `sysconfdir` -> `/etc` (build
prefix, like rbconfig) vs CRuby's install-prefix path — rubygems rescues it.

**Current blocker (STDLIB):** `cannot load such file -- fileutils` (via
`rubygems/config_file.rb`). The require graph now clears config_file.rb past
`Etc`; **`fileutils`** is the biggest pure-Ruby stdlib (40 requires; `mkdir_p`/
`cp_r`/`rm_rf`/`mv` over File/Dir) — vendor it into `gems/`.

**Missing pure-Ruby stdlib — vendor into `gems/`** (each sits on File/Dir/
Process/IO that zeo largely has; ordered by the `bundle --version` → install
path):

| Feature | Why | Notes |
|---|---|---|
| `fileutils` | gem install core; **40** requires | biggest; `mkdir_p`/`cp_r`/`rm_rf`/`mv` over File/Dir |
| `pathname` | bundler uses `Pathname` pervasively (`Bundler.root`) | pure Ruby over File |
| `tempfile` | atomic writes, downloads | sits on `Dir.mktmpdir` (already present) |
| `open3` | subprocess with pipes (`capture3`) | over Process/IO |
| `find` | dir walker | small |
| `logger` | Bundler's logging | pure Ruby |
| `erb` | template rendering | pure Ruby, non-trivial |
| `uri` (top-level) | bundler sources | rubygems uses its OWN vendored copy; bundler wants top-level |
| `open-uri` + `net/http` + `resolv` | **M3** network install | biggest; TLS via ext-openssl growth |

**Missing NATIVE ext stdlib:**
- `etc` — ✅ DONE (full native `ext/etc`).
- `io/wait` — `IO#wait_readable`/`#wait_writable` (net/http).
- `mkmf` — native-extension Makefile generation. **M2/M3**, large; only needed to
  install gems with C extensions, not for `bundle --version`.

**Lowering gaps in ALREADY-vendored gems** (surfaced now that the graph reaches
them; each small):
- `rescue *yaml_errors => e` (`rubygems.rb:410`) — ✅ FIXED (splat-rescue).
- `shellwords.rb:66` — a module-level construct not lowered (triage).
- `delegate.rb:47` — `alias __raise__ raise` (alias of an inherited builtin —
  the partially-handled alias family).
- `forwardable.rb:213` — `loc, = caller_locations(2,1)` — ✅ FIXED (the multi-
  assign trailing-comma fix above).
- `expr::CONST` / ScopedConstRead — still open, 1 site, small.

## Sequencing recommendation

1. **Dynamic `load` → runtime-LoadError lowering** (unblocks the current probe).
2. **Grind the missing pure-Ruby stdlib**, vendoring one at a time and re-probing
   (`fileutils` and `pathname` are the highest-leverage for the bundle path).
3. **Small lowering gaps** (`loc, =`, alias-of-builtin, ScopedConstRead,
   shellwords) opportunistically as the graph hits them.
4. **Native `etc`/`io/wait`** when the graph demands them.
5. `mkmf`, `open-uri`/`net/http` and rbconfig target-arch fidelity are **M2/M3**
   (real gem/network install), not `bundle --version`.

Milestones stay independent of the conformance percentage; track the first-N
error trend here as the graph opens up.
