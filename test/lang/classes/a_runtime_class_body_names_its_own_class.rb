# Ruby binds a class's constant BEFORE running its body, so a class whose
# superclass is a runtime value may name itself inside that body. jmespath's
# token.rb is exactly this shape:
#
#   class Token < Struct.new(:type, :value, :position, :binding_power)
#     NULL_TOKEN = Token.new(:eof, '', nil)
#
# zeo builds such a class as `Name = Class.new(sup) { body }`, which assigns the
# constant only after the body has run -- so the self-reference raised
# `uninitialized constant`. In the body `self` IS the class, which is the exact
# answer; inside a `def` the constant is bound by the time the method can run,
# so that read keeps the ordinary lexical path.
module M
  class T < Struct.new(:a, :b)
    FIRST = T.new(1, 2)
    LATER = T.new(3, 4)

    def self.build(x)
      T.new(x, x * 2)
    end

    def sibling
      T.new(a + 1, b + 1)
    end
  end

  # A plain class names itself the same way, and always could.
  class P
    SELF_REF = P
  end
end

p M::T::FIRST.a
p M::T::LATER.b
p M::T.build(5).b
p M::T::FIRST.sibling.a
p M::P::SELF_REF
p M::T::FIRST.is_a?(M::T)
__END__
1
4
10
2
M::P
true
