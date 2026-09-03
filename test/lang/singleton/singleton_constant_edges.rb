# Singleton-class constants (`class << self` surrogates) beyond the promoted
# gap shape: reopens, enclosing-scope reads, nesting depth, and the reach
# rules from outside the singleton body.

# The enclosing module's OWN constants stay readable from the singleton body
# (it sits in the lexical nesting), and the singleton's shadow the module's.
module Outer
  PLAIN = :module_const
  SHADOWED = :from_module
  class << self
    SC = :singleton_const
    SHADOWED = :from_singleton
    def both = [SC, PLAIN, SHADOWED]
  end
end
p Outer.both

# A class method defined OUTSIDE the `class << self` body cannot reach the
# singleton's constant by bare name.
module Outer
  def self.blind
    SC
  rescue NameError
    :name_error
  end
end
p Outer.blind

# A second `class << self` body reopens the SAME singleton class -- its
# constants join the first body's.
module Outer
  class << self
    SC2 = :second_body
    def pair = [SC, SC2]
  end
end
p Outer.pair
p Outer.singleton_class.constants(false).sort

# Nested modules: the surrogate sits inside the full lexical chain, so a
# singleton body in `A::B` reads A's constants too.
module A
  ROOT = :a_const
  module B
    class << self
      INNER = :inner
      def chain = [INNER, ROOT]
      def nest = Module.nesting.map(&:to_s)
    end
  end
end
p A::B.chain
p A::B.nest

# Reflection through the singleton value: the constant is reachable with
# const_get on the singleton class, and invisible from the module itself.
p Outer.singleton_class.const_get(:SC)
p [Outer.const_defined?(:SC), Outer.singleton_class.const_defined?(:SC)]
__END__
[:singleton_const, :module_const, :from_singleton]
:name_error
[:singleton_const, :second_body]
[:SC, :SC2, :SHADOWED]
[:inner, :a_const]
["#<Class:A::B>", "A::B", "A"]
:singleton_const
[false, true]
