# Issue #229: an unused class method whose body has bare `new(args)`
# against a defining class with different `initialize` arity used to
# emit a body that wouldn't C-compile. Sister to issue #224 (which
# fixed bare-`new` *dispatch* for reachable inherited cls methods);
# this is the unreached-emit complement. The fix DCEs cls methods
# with no call site so unreachable bodies never have to type-check.
#
# Three Sam-Ruby-filed variants exercise here: solo class (arity
# mismatch), 3-class inheritance with one leaf called (multiple
# dead bodies multiply), and default-arg initialize (type mismatch).

# Variant 1: solo class. Base.create is defined but never called.
# Pre-fix: `sp_Base_cls_create` emits `return sp_Base_new(lv_attrs)`
# against a 0-arg `sp_Base_new(void)` and `cc` rejects.
class Base
  def initialize
    @x = 1
  end
  def x; @x; end

  def self.create(attrs)
    new(attrs)
  end
end

b = Base.new
puts b.x                               # 1

# Variant 2: 3-class inheritance, only the leaf is called.
# Pre-fix all three (Base2/Mid/Leaf) emitted cls_create bodies;
# only Leaf's compiled. With DCE, only Leaf::create is live.
class Base2
  def initialize
    @v = 0
  end

  def self.create(attrs)
    new(attrs)
  end
end

class Mid < Base2
end

class Leaf < Mid
  def initialize(x)
    @x = x
  end
  def x; @x; end
end

# `Leaf.create(42)` runs the body propagated from Base2.create:
# `instance = new(attrs)` => `Leaf.new(42)` => Leaf#initialize(42).
puts Leaf.create(42).x                 # 42

# Variant 3: default-arg initialize. `sp_Base3_new` is synthesized
# as `sp_Base3_new(sp_StrIntHash *)` from the default literal, but
# the uncalled `Base3.create` can't infer its `attrs` ptype so the
# body's `sp_Base3_new(lv_attrs)` had `lv_attrs : mrb_int` against
# the declared `sp_StrIntHash *`. DCE drops the body entirely.
class Base3
  def initialize(attrs = {})
    @v = attrs.length
  end
  def v; @v; end

  def self.create(attrs)
    new(attrs)
  end
end

# Demonstrate Base3 still works for the *called* path: explicit
# `Base3.new` is live, so `sp_Base3_new` is emitted and called.
# Only `Base3.create` (the uncalled cls method) drops.
b3 = Base3.new
puts b3.v                              # 0
