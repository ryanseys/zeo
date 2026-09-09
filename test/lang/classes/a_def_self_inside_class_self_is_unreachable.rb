# A `def self.x` written INSIDE `class << self` defines a method on the
# singleton's OWN singleton -- `K.singleton_class.x` -- and it is NOT a class
# method of `K`.
#
# `class << self`'s body is retagged onto the enclosing class, so a plain `def`
# there becomes a class method (see `zeo-singleton-model`). Retagging a
# `def self.x` the same way put it one level too low, which was two divergences
# from one drop: `K.x` answered where ruby raises NoMethodError, and
# `K.singleton_class.x` was undefined.
#
# The level it belongs to has no compile-time namespace in zeo, and needs
# none: a singleton class is an ordinary OBJECT at run time, so the def is a
# per-object singleton method on it -- exactly what `class << obj` already
# spells, through the same `define_singleton_method` lowering. `class << obj`
# had the same hole and takes the same route.
#
# The wider sweep is `test/lang/singleton/singleton_chain_dispatch_sweep.rb`.

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
__END__
"class method"
"meta"
