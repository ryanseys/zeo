# A module prepended into a SINGLETON class, reached from every direction a
# subclass chain gives it.
#
# zeo has no singleton-class objects: a class method resolves through the
# `class_method_prepends` stack, the extend sources and the flattened
# `class_methods` tables. Three things that model got wrong, and each needed a
# different half of the answer.
#
# `defined?(super)` and the `super` WALK disagreed. The probe scanned from the
# target's own position, which INCLUDES the module prepended into its
# singleton, while the walk skips that layer -- it got there THROUGH it. A
# prepended module with no row beneath it therefore reported a super target
# and then raised looking for it. One resolution serves both now.
#
# A SUBCLASS did not inherit its parent's singleton prepend for DISPATCH, only
# for reflection: `prepended_class_methods` is keyed on the class the
# `prepend` named, and the flat probe found the parent's own `def self.x`
# instead -- which sits BEHIND the module in ruby's chain.
#
# And a prepend on an ANCESTOR was unreachable by `super` from a prepend on
# the subclass, because materialization flattens an inherited `def self.x`
# onto every descendant's table and the walk took the nearest copy. Only the
# OWN-set can tell a class's real definition from the copy it was given.

module L1
  def tag = "L1(#{defined?(super) ? super : 'top'})"
end
module L2
  def tag = "L2(#{defined?(super) ? super : 'top'})"
end

# A prepend with nothing beneath it: `defined?(super)` is false, and the
# fallback branch runs rather than raising.
class A1
  singleton_class.prepend L1
end
p A1.tag

# ... and with the host's own row beneath it.
class A2
  def self.tag = "own"
  singleton_class.prepend L1
end
p A2.tag

# A subclass inherits the prepend, at its parent's position in the chain.
class B0
  def self.tag = "B0"
end
class B1 < B0; end
class B2 < B1; end
B0.singleton_class.prepend L1
p [B0.tag, B1.tag, B2.tag]

# A prepend on the SUBCLASS resumes into the one on the ancestor.
B2.singleton_class.prepend L2
p [B0.tag, B1.tag, B2.tag]
p B2.singleton_class.ancestors.first(5).map(&:to_s)

# A subclass's OWN `def self.x` still outranks an ancestor's prepend, and its
# `super` reaches it.
class B3 < B0
  def self.tag = "B3(#{super})"
end
p B3.tag
p B3.singleton_class.ancestors.first(4).map(&:to_s)

# An `extend` of the same module is a different layer, and the two orders
# answer differently.
class C1
  extend L1
end
p C1.tag
class C2
  def self.tag = "own2"
  extend L1
end
p C2.tag

# The walk's tail is `Class`'s own instance methods.
class D1
  def self.to_s = "d1(" + super + ")"
end
p D1.to_s
__END__
"L1(top)"
"L1(own)"
["L1(B0)", "L1(B0)", "L1(B0)"]
["L1(B0)", "L1(B0)", "L2(L1(B0))"]
["L2", "#<Class:B2>", "#<Class:B1>", "L1", "#<Class:B0>"]
"B3(L1(B0))"
["#<Class:B3>", "L1", "#<Class:B0>", "#<Class:Object>"]
"L1(top)"
"own2"
"d1(D1)"
