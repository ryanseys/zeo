# `obj.extend(A, B)` layered B nearest the object, where CRuby layers A.
#
# CRuby's `rb_obj_extend` (`object.c`) walks its arguments BACKWARDS -- `argc-1`
# down to `0` -- and each `extend` inserts above the one before it, so the
# FIRST argument ends up closest. zeo's `Kernel#extend` walked forwards, which
# reverses the chain, and with it which module wins a name both define.
#
# `Module#include(A, B)` has the same backwards rule and was already right;
# only the object-side `extend` walked the wrong way.

module A1
  def who = :a
  def only_a = :only_a
end
module B1
  def who = :b
  def only_b = :only_b
end

o = Object.new.extend(A1, B1)
p o.singleton_class.ancestors[1, 2].map(&:to_s)
p o.who
p [o.only_a, o.only_b]

class K; end
K.extend(A1, B1)
p K.singleton_class.ancestors[1, 2].map(&:to_s)
p K.who

# Two separate calls layer in call order: the LATER one wins.
q = Object.new
q.extend(A1)
q.extend(B1)
p q.singleton_class.ancestors[1, 2].map(&:to_s)
p q.who

# `include` was already right, and stays right.
class M1
  include A1, B1
end
p M1.ancestors[1, 2].map(&:to_s)
p M1.new.who
