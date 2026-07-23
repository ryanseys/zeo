# Bundler north star — compile-`bundle` inventory

**Goal (plan Phase 6 §G):** zeo compiles `rubygems` + `bundler` out of the box,
milestones `bundle --version` → `bundle install --local` → network install.

**Status (2026-07-23): does NOT compile yet.** rubygems fails during its own
require graph. This is the first real attempt; the blockers below are the
worklist.

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

### 3. Non-top-level `require` — architectural — **THE dominant blocker (~252 sites)**
`require "securerandom" unless defined?(SecureRandom)`
(`rubygems/util/atomic_file_writer.rb:15`) and ~252 more: a `require` inside a
method / conditional / module / `begin`. zeo only accepts a top-level
single-string-literal `require` (resolved at compile time).

Because zeo links everything statically anyway, the fix is to **hoist a
non-top-level literal `require` to a compile-time splice** (resolve + lower the
target eagerly, as if top-level) and leave the runtime `require` call a no-op
returning true/false. The surrounding `unless defined?(X)` guard becomes
harmless — X is already defined after the eager splice. Over-approximation (a
require in a never-taken branch still compiles in) is fine for AOT. Design
decision needed: dedup/ordering, and requires with a NON-literal arg (only 2
sites — can stay a clean rejection).

### 4. `autoload` — architectural — **~102 sites**
`autoload :Foo, "path"` registers a const→file mapping loaded on first const
access. Same shape of fix as #3: at compile time, treat `autoload` as an eager
compile-in of the target file + a normal constant, since everything is linked.

## Sequencing recommendation

1. **#3 non-top-level require hoisting** — unblocks the most, and is a
   prerequisite for essentially everything downstream. Design + implement first.
2. **#4 autoload** — same mechanism; do together with #3.
3. **#2 ScopedConstRead** — small, do opportunistically.
4. Re-probe: expect a long tail of individual lowering gaps + missing
   stdlib/`Gem`/`Bundler` runtime surface once the require graph loads. Catalog
   them here as they appear (this is the M1 `bundle --version` grind).
5. rbconfig target-arch fidelity (compile-time-generated) — needed for real gem
   install (M2/M3), not for `--version`.

Milestones stay independent of the conformance percentage; track the first-N
error trend here as the graph opens up.
