# `Ruby::Box.new` answers a value whose `.class` is `Module`, so it responds
# to none of `Ruby::Box`'s own methods -- and it answers one at all without
# `RUBY_BOX=1`, which ruby refuses outright.
#
# Two bugs, one value:
#
#   the ENV GATE. CRuby raises `RuntimeError: Ruby Box is disabled. Set
#   RUBY_BOX=1 ...` from `Ruby::Box#initialize`. zeo's runtime already has
#   `boxes::disabled_error()` and the DYNAMIC path already raises it; the
#   COMPILE-TIME path (`parse/loader.rs`, where a top-level `Ruby::Box.new`
#   is turned into a compile-time box) never asks.
#
#   the HANDLE's class. In CRuby `Ruby::Box` IS a `Module` subclass, so
#   zeo's `RubyValue::Class(surrogate)` representation is structurally
#   RIGHT -- `b::W` resolves for the same reason. Only `.class` is wrong: it
#   answers `Module`, so `b.is_a?(Ruby::Box)` is false and `b.eval` silently
#   resolves `Kernel#eval` instead of `Ruby::Box#eval`. `value/mod.rs`'s
#   class-of already consults `runtime_meta::module_owner_class` and
#   `dispatch::class_is_refinement` before falling back to `MODULE_CLASS`;
#   this wants one more arm there.
#
# The gate cannot be shown here -- the golden harness runs the oracle with
# `RUBY_BOX=1` whenever a source mentions `Ruby::Box` -- so this file pins
# the handle's identity, and the gate is recorded above.
#
# Both are G7's B0.
#
# Oracle: the handle is a `Ruby::Box`, and it is not the main box.
b = Ruby::Box.new
p b.class.to_s
p b.is_a?(Ruby::Box)
p b.main?
