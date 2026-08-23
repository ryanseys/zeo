# Every order a class body can put a run-time singleton edit and a
# compile-time `extend` in. The rule is CRuby's own: `rb_include_module`
# searches the whole chain and `rb_prepend_module` only the prepend area, so
# whichever verb runs FIRST finds an empty scope and the other may find its
# copy already there.
#
# Four of these were wrong before a class-body `extend` was given a document
# position (see a_class_body_extend_is_applied_before_the_body_runs.rb): A, E,
# F and I. C, D and G were already right, which is why the gap file named one
# shape and the sweep found four.

module M
  def hi = "m"
end
module N
  def bye = "n"
end

def chain(k) = k.singleton_class.ancestors.map(&:to_s).first(5)

class A
  singleton_class.prepend M
  extend M
end
p chain(A); p A.hi; p A.is_a?(M); p A.singleton_class.include?(M)

class B
  extend M
  singleton_class.prepend M
end
p chain(B); p B.hi

class C
  singleton_class.include M
  extend M
end
p chain(C); p C.hi

class D
  singleton_class.extend M
  extend M
end
p chain(D); p D.hi

class E
  singleton_class.prepend M
  extend N
  extend M
end
p chain(E); p E.hi; p E.bye

class F
  singleton_class.prepend M
  singleton_class.prepend N
  extend M
  extend N
end
p chain(F)

module G
  singleton_class.prepend M
  extend self
  def hi = "g"
end
p chain(G); p G.hi

class H
  extend M
  extend M
end
p chain(H)

class Base
  singleton_class.prepend M
  extend M
end
class Sub < Base; end
p chain(Sub); p Sub.hi

class I
  singleton_class.prepend M
  include M
  extend M
end
p chain(I); p I.ancestors.map(&:to_s).first(3); p I.new.hi
