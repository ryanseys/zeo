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
# The fix shape for the rest is a singleton ancestry that is a real linearized
# chain -- the same `include_modules_at` replay the instance side runs -- with
# positions to walk, rather than branches over side tables.

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
