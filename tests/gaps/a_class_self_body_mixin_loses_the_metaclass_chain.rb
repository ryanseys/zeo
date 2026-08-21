# A `class << self` body that mixes a module in truncates the singleton class's
# ancestry to `Object`'s chain, and an `include` there is dropped outright.
#
# Found 2026-08-21 by the combinatorial sweep in
# `tests/mixins_across_classes_modules_and_singletons.rb`. Two divergences,
# one cause:
#
#   class Bare; end
#     -> zeo is CORRECT: the runtime mints the singleton and
#        `singleton_super_chain` walks ruby's parallel metaclass hierarchy
#        (`#<Class:Bare>`, `#<Class:Object>`, `#<Class:BasicObject>`, `Class`,
#        `Module`, `Object`, ...).
#
#   class One; class << self; prepend SA; end; end
#     -> the mixin makes the compiler register a SINGLETON SURROGATE with an
#        ancestry of its own, computed by `analyze::mro::compute_ancestors`,
#        which roots every class at `Object`. So `SA` and `#<Class:One>` are
#        right and everything after them is the ORDINARY chain: `Object`,
#        `Kernel`, `BasicObject`, where ruby continues through `#<Class:Object>`.
#
#   class Three; class << self; include SA; end; end
#     -> the same, plus `SA` is missing entirely and
#        `Three.singleton_class.include?(SA)` answers false. `class << self`'s
#        `prepend` lowers to `HirNode::ClassMethodPrepend`, which the class-body
#        walk records; its `include` has no such arm.
#
# Dispatch is UNAFFECTED in both -- `One.s` and `Three.s` answer what ruby
# answers, and the sweep's `super` chains all agree. This is the reflection
# surface alone.
#
# The fix shape: the surrogate's ancestry must be built the way
# `runtime_meta::singleton_super_chain` builds a minted one (the parallel
# metaclass chain), not by `compute_ancestors`; and the class-body walk needs
# the `include` arm its `prepend` sibling has.
module SA; def s = "SA"; end

class Three
  def self.s = "Three"
  class << self
    include SA
  end
end
p Three.singleton_class.include?(SA)
p Three.singleton_class.ancestors.map(&:to_s)

class One
  def self.s = "One"
  class << self
    prepend SA
  end
end
p One.singleton_class.ancestors.map(&:to_s)
