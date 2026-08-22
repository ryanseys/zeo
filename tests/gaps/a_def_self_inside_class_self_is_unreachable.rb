# A `def self.x` written INSIDE `class << self` defines a method on the
# singleton's own singleton -- `K.singleton_class.x` -- and zeo drops it.
#
# `class << self`'s body is retagged onto the enclosing class: a plain `def`
# there becomes a class method, which is the whole of what the splice does
# (see `zeo-singleton-class-model`). A `def self.x` inside it is one level
# further up, on `#<Class:#<Class:K>>`, and the retagging has nowhere to put
# it -- zeo has no singleton-class OBJECTS, so there is no second level to
# retag onto.
#
# Reproduces with and without an `eval` around it, so it is not the body-slice
# question `tests/a_singleton_body_inside_an_eval_keeps_its_end.rb` closed.
#
# The fix shape is the same one the doubled-module singleton gap needs: a
# singleton ancestry that is a real linearized chain of real class ids, so
# `#<Class:K>` is a class like any other and its own singleton is one more.

class K
  class << self
    def hi = "class method"
    def self.meta = "meta"
  end
end
p K.hi
begin
  p K.singleton_class.meta
rescue NoMethodError => e
  p e.class.to_s
end
