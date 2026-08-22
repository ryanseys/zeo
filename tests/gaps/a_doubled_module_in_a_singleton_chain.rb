# A module both INCLUDED and PREPENDED into a class's SINGLETON class holds
# two positions there, exactly as it does in an instance ancestry -- and its
# body runs once per position.
#
# The instance side answers this (see
# `tests/a_module_both_prepended_and_included.rb`): the chain carries the
# duplicate and `dispatch::MRO_RESUME` tells a `super` which copy is running.
# The CLASS-METHOD side is a different mechanism and is not covered by it.
# zeo has no singleton-class objects, so a class method resolves through
# `send_super_class_from`'s three branches over `class_method_prepends`, the
# extend sources and the flattened `class_methods` tables, never over a
# singleton ancestry `Vec`. There is nothing there to hold a module twice.
#
# What the singleton chain DOES answer now, and did not: a prepended module
# with no row beneath it (its `super` and its `defined?(super)` used to
# disagree, because the probe scanned the layer the walk skips); a subclass
# inheriting its parent's singleton prepend; and a prepend on an ANCESTOR
# reached by `super` from a prepend on the subclass. Those needed one
# resolution shared by the call and the probe, plus the own-set to tell a
# class's real `def self.x` from the copy materialization gave it.
#
# HALF OF IT LANDED 2026-08-22, and the file is down to ONE diverging line.
# `runtime_meta::singleton_chain` builds the two areas apart, so both
# `singleton_class.ancestors` rows below are byte-identical to ruby and hold
# the module twice. P1's dispatch was already right. What is left is P2's, and
# it is the SystemStackError.
#
# WHAT IS LEFT, mapped. `send_super_class_from(P2, CM, :hi)` finds CM in no
# instance ancestor, sees `has_singleton_prepend(P2, CM)`, finds no other
# prepend below it, and re-enters `send_class_walking_inner` with
# `first_below_prepends`. That skips the PREPENDED copy and finds the EXTENDED
# one -- correct -- but the extended copy's baked `defining_class` is also CM,
# so its own `super` takes the identical branch and re-enters forever. Both
# copies name the same module; only their POSITION differs, and the walk has
# none.
#
# The instance side solved exactly this with `dispatch::MRO_RESUME`: the walk
# that enters a body publishes `(defining_class, next_index)` and `super_resume`
# reads it back, so a `super` resumes past the copy that is RUNNING rather than
# past the first one. The class-method channel needs the same fact, and needs
# positions to number:
#
#   * a derived singleton walk -- for each non-module ancestor, its singleton
#     prepends (latest first), then its own `def self.x` layer, then its
#     extends (latest first). That is CRuby's parallel metaclass chain, and it
#     is the order `scan_class_method_owner` already half-walks;
#   * `resolve_class_walking` returning the POSITION it hit as well as the hit,
#     and `send_class_walking_inner` publishing it around the call;
#   * its own resume cell rather than `MRO_RESUME`. The two channels index
#     different sequences, and a module used BOTH as an instance mixin and a
#     singleton one would have them collide on `defining_class`. A second cell
#     joins the `Ec` bundle, as the first did;
#   * `super_class_defined` reading the same resolution, or the probe and the
#     walk drift again -- which is the shape this file has already hit twice.

module CM
  def hi = "cm(#{defined?(super) ? super : 'top'})"
end

class P1
  extend CM
  singleton_class.prepend CM
  def self.hi = "own(#{defined?(super) ? super : 'top'})"
end
p P1.singleton_class.ancestors.map(&:to_s)
p P1.hi

class P2
  singleton_class.include CM
  singleton_class.prepend CM
end
p P2.singleton_class.ancestors.map(&:to_s)
p P2.hi
