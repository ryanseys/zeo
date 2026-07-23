# Bundler north star — compile-`bundle` inventory

**Goal (plan Phase 6 §G):** zeo compiles `rubygems` + `bundler` out of the box,
milestones `bundle --version` → `bundle install --local` → network install.

**Status (2026-07-23): does NOT compile yet, but the architecture is now in
place.** The two dominant architectural blockers — `rbconfig` and non-top-level
`require` — are fixed; rubygems now loads its require graph until it hits a
MISSING STDLIB (`securerandom`). What remains is the grind: implement/vendor the
stdlib + `Gem`/`Bundler` runtime surface, one missing feature at a time.

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

## New / remaining blockers — the grind begins

- **Missing stdlib `securerandom`** (current blocker) — there is no
  `SecureRandom` in zeo at all (the plan's "SecureRandom → OS entropy" is
  unimplemented). Needs a builtin (`getrandom` is already linked for openssl) or
  a vendored stdlib file.
- **`loc, = expr`** — a single-target multi-assignment with a trailing comma
  (destructure first element), `gems/forwardable/lib/forwardable.rb:213`. A small
  lowering gap, surfaced now that nested requires actually load forwardable.
- **#2 `expr::CONST`** (ScopedConstRead) — still open, 1 site, small.

## Sequencing recommendation

1. **Grind the missing stdlib + `Gem`/`Bundler` runtime surface** one blocker at
   a time (`securerandom` next), re-probing after each. This is the M1
   `bundle --version` path; catalog each here as it appears.
2. **#2 ScopedConstRead** and **`loc, =`** — small lowering gaps, do
   opportunistically when hit.
3. rbconfig target-arch fidelity (compile-time-generated) — needed for real gem
   install (M2/M3), not for `--version`.

Milestones stay independent of the conformance percentage; track the first-N
error trend here as the graph opens up.
