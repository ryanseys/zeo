# Vendored-stdlib compile gaps (probe log)

Ten pure-Ruby default gems from ruby 4.0.5 are vendored under `gems/`
(see `gems/UPSTREAM.md`). Requiring each in a probe program (2026-07-21)
gives the real remaining blocker per gem -- these are compiler work items,
now surfaced honestly instead of `cannot load such file`:

| gem | status | blocker |
|---|---|---|
| tsort | **works** | -- |
| ostruct | blocked | `alias_method :raise!, :raise` -- alias of an INHERITED builtin (Kernel) on a user class; plus the bulk `instance_methods.each { alias_method ... }` loop; attribute misses then need `method_missing` dispatch (plan M8) |
| delegate | blocked | `alias __raise__ raise` -- same alias-of-inherited-builtin family |
| shellwords | blocked | `class << self` body with `extend`/ivars (lowering handles only defs/constants/include/attr_*/private/alias) |
| English | blocked | `alias $FULL_MATCH $&` -- alias of punctuation globals (lowering accepts only plain `$name` pairs) |
| forwardable | blocked | a multi-assignment splat-target shape lowering rejects (`expected *name as a multi-assignment's splat target`) |
| pp | blocked | `require` in non-top-level position (inside method/conditional) |
| timeout | blocked | generated Rust fails rustc -- codegen bug, triage via the kept temp source |
| prettyprint | blocked | generated Rust fails rustc -- codegen bug, triage via the kept temp source |
| singleton | blocked | compiler PANIC during compile (run with RUST_BACKTRACE=1) -- highest-priority of these: panics are never acceptable diagnostics |

Grind order suggestion: the singleton panic first (compiler bug class),
then the two rustc-failure codegen bugs, then the alias-of-inherited
family (unblocks ostruct + delegate together), then the lowering shapes
(class << self extend, global aliases, splat target, nested require).
Re-probe each gem after its blocker lands; move rows out of this table as
they turn green, and delete the file when empty.
