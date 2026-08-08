# The corpus's single biggest lowering gap: `class << self` bodies rejected on
# ONE shape -- a call with a receiver whose BLOCK reaches for `self`:
#
#     class << self
#       %w(get post).each { |n| define_method(n) { ... } }
#     end
#
# The mapping worked by rewriting a statement's RECEIVER, which cannot rebind
# `self` for a nested block, so the whole statement was refused.
#
# It now runs where ruby runs it: as a statement of the singleton's own class
# body. That is CRuby's structure, not a workaround for it -- `NODE_SCLASS`
# compiles to a child iseq whose `self` IS the singleton class object, so a
# block created inside sees that `self` for free.
$log = []
class Ordered
  class << self
    $log << :before_def
    def a = :a
    $log << :after_def
    %w(x y).each { |n| define_method(n) { n } }
    $log << :after_each
    def b = :b
    $log << :after_b
  end
end

# Statements run in SOURCE order, and the runtime one keeps its position among
# them.
p $log
p [Ordered.a, Ordered.b, Ordered.x, Ordered.y]

# The methods are real class methods of the enclosing class, reachable from a
# statically-compiled call site -- which is the whole question the design
# turned on.
p Ordered.respond_to?(:x)
p Ordered.singleton_class.method_defined?(:x)

# `singleton_method_added` fires for a `define_method`'d name exactly as for a
# `def`, and in source order -- which is what makes the ordering above an
# observable rather than a tidiness argument.
$hooks = []
class Hooked
  def self.singleton_method_added(n) = $hooks << [:sma, n]
  class << self
    [:viaeach].each { |n| define_method(n) { n } }
    def plain = :plain
  end
end
p $hooks

# Reflection cannot tell the two apart: same owner, same list.
class Refl
  class << self
    [:m1].each { |n| define_method(n) { n } }
    def m2 = :m2
  end
end
p Refl.singleton_class.instance_methods(false).sort
p (Refl.methods - Object.methods).sort
p Refl.singleton_class.instance_method(:m1).owner == Refl.singleton_class
p Refl.method(:m1).owner == Refl.singleton_class

# The block's `self` is the singleton, so a `def_delegator`-style macro or any
# other receiverless send inside it targets the singleton too.
class Macro
  class << self
    def helper(n) = "helped:#{n}"
    [:one, :two].each do |n|
      define_method(n) { helper(n) }
    end
  end
end
p [Macro.one, Macro.two]

# Constants in the same body still belong to the singleton class, and the
# runtime statement can read them.
class WithConst
  class << self
    NAMES = %w(alpha beta)
    NAMES.each { |n| define_method(n) { n } }
  end
end
p [WithConst.alpha, WithConst.beta]
p WithConst.singleton_class.const_defined?(:NAMES)
p WithConst.const_defined?(:NAMES)
