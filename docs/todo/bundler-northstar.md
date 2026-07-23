# Bundler north star — compile-`bundle` inventory

**Goal (plan Phase 6 §G):** zeo compiles `rubygems` + `bundler` out of the box,
milestones `bundle --version` → `bundle install --local` → network install.

**Status (2026-07-23): does NOT compile yet, but the architecture is now in
place.** The dominant architectural blockers — `rbconfig`, non-top-level
`require`, and now `securerandom` (with `Random::Formatter` + OS-entropy
`Random.urandom` + `extend`-onto-class/module) — are fixed. rubygems now loads
its require graph past atomic_file_writer, stopping at a dynamic `load`
(`rubygems.rb:293`). What remains is the grind: one compiler over-approximation
(dynamic `load`) then vendoring/implementing the missing stdlib
(`fileutils`/`pathname`/... — see the worklist), one feature at a time.

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

**Current blocker (compiler, NOT stdlib):** dynamic `load` with a non-literal
target — `load ENV["BUNDLE_BIN_PATH"] if ...` (`rubygems.rb:293`). Whole-program
AOT can't splice a runtime-computed path. Fix: lower a non-literal `load`/
`require` to a runtime op that raises `LoadError` when actually executed (an
over-approximation), instead of rejecting the whole compile. The site is guarded
(`if ENV[...]`), so at runtime it's a no-op on the normal path.

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
- `etc` — `Etc.sysconf`/passwd/`nprocessors` (rubygems reads the user gem dir).
- `io/wait` — `IO#wait_readable`/`#wait_writable` (net/http).
- `mkmf` — native-extension Makefile generation. **M2/M3**, large; only needed to
  install gems with C extensions, not for `bundle --version`.

**Lowering gaps in ALREADY-vendored gems** (surfaced now that the graph reaches
them; each small):
- `shellwords.rb:66` — a module-level construct not lowered (triage).
- `delegate.rb:47` — `alias __raise__ raise` (alias of an inherited builtin —
  the partially-handled alias family).
- `forwardable.rb:213` — `loc, = caller_locations(2,1)` (single-target
  multi-assign with a trailing comma).
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
