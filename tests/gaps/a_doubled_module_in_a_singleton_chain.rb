# A module both INCLUDED and PREPENDED into a class's SINGLETON class holds
# two positions there, exactly as it does in an instance ancestry -- and its
# body runs once per position.
#
# The instance side answers this (see
# `tests/a_module_both_prepended_and_included.rb`): the chain carries the
# duplicate and `dispatch::MRO_RESUME` tells a `super` which copy is running.
# The CLASS-METHOD side is a different mechanism and is not covered by it.
# zeo has no singleton-class objects, so a class method resolves through
# `send_super_class_from`'s three branches over `class_method_prepends` and
# the flattened `class_methods` tables, never over a singleton ancestry
# `Vec`. There is nothing there to hold a module twice, and the branch that
# resumes a singleton-prepended module's `super` finds the same copy again.
#
# Two symptoms, and the difference between them says where the fix goes:
#
#   * `extend M` + `singleton_class.prepend M` ANSWERS correctly
#     (`cm(own(cm(top)))`) because the flattening already puts the module on
#     both sides of the host's own row -- only `singleton_class.ancestors`
#     lists it once.
#   * `singleton_class.include M` + `singleton_class.prepend M` RECURSES
#     until `SystemStackError`: with no own `def self.x` between them, the
#     prepended copy's `super` resolves back to itself.
#
# Pre-existing; the instance-side MRO pass neither caused nor fixed it. The
# fix shape is a singleton ancestry that is a real linearized chain -- the
# same `include_modules_at` replay the instance side now runs -- rather than
# three special-cased branches over two side tables.

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
