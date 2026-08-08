# The single biggest lowering gap in the corpus: 1,350 rows over 383 files.
# `class << self` bodies overwhelmingly reject on ONE shape -- a call with a
# receiver whose BLOCK reaches for `self`:
#
#     class << self
#       %w(get post).each { |n| define_method(n) { ... } }
#     end
#
# `map_class_self_items` maps each statement by rewriting its RECEIVER, which
# cannot rebind `self` for a nested block, so the statement is rejected.
#
# This file records what CRuby actually does, so the fix has a target rather
# than an intention. Every assertion here is oracle output.
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

# Statements run in SOURCE order. zeo hoists `def`s to compile time, which is
# invisible here -- but the runtime statement between them is not, and neither
# are the hooks below.
p $log
p [Ordered.a, Ordered.b, Ordered.x, Ordered.y]

# A `private` directive is a CURSOR that persists across the block: methods
# `define_method` installs after it are private singleton methods. So a design
# that runs the residual statement somewhere else has to carry the visibility
# state there too, not just the receiver.
class Vis
  class << self
    private
    [:hidden].each { |n| define_method(n) { :h } }
    public
    [:shown].each { |n| define_method(n) { :s } }
  end
end
p Vis.singleton_class.private_method_defined?(:hidden)
p Vis.singleton_class.public_method_defined?(:shown)
p (Vis.hidden rescue $!.class)
p Vis.shown

# `singleton_method_added` fires for a `define_method`'d name exactly as for a
# `def`, and in source order -- which pins the ordering constraint above to an
# observable, not just a tidiness argument.
$hooks = []
class Hooked
  def self.singleton_method_added(n) = $hooks << [:sma, n]
  class << self
    [:viaeach].each { |n| define_method(n) { n } }
    def plain = :plain
  end
end
p $hooks

# Reflection cannot tell the two apart: same owner, same list, same order.
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
