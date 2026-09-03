# A `def` written in a constant-bearing `class << self` body is lexically
# inside the SINGLETON class. Its bare constants and `Module.nesting` resolve
# through the surrogate, whose lexical parent is the class itself -- so the
# rest of the chain is unchanged, and dispatch/ivars/`super` keep using the
# owner.

module Outer
  PLAIN = :module_const
  SHADOWED = :from_module
  class << self
    SC = :singleton_const
    SHADOWED = :from_singleton
    def both = [SC, PLAIN, SHADOWED]
    def nest = Module.nesting.map(&:to_s)
  end
end
p Outer.both
p Outer.nest

# A class method written OUTSIDE the singleton body cannot see the
# singleton's constant by bare name.
module Outer
  def self.blind
    SC
  rescue NameError
    :name_error
  end
end
p Outer.blind

# Nesting depth: the surrogate joins the full lexical chain.
module A
  ROOT = :a_const
  module B
    INNER_MOD = :b_const
    class << self
      INNER = :inner
      def chain = [INNER, INNER_MOD, ROOT]
      def nest = Module.nesting.map(&:to_s)
    end
  end
end
p A::B.chain
p A::B.nest

# `Module.nesting` outside any body, and in an ordinary class body.
p Module.nesting
class Plain
  NEST = Module.nesting.map(&:to_s)
  def self.nest = Module.nesting.map(&:to_s)
end
p Plain::NEST
p Plain.nest

# Reflection agrees: the constant lives on the singleton, not the module.
p Outer.singleton_class.const_get(:SC)
p [Outer.const_defined?(:SC), Outer.singleton_class.const_defined?(:SC)]
__END__
[:singleton_const, :module_const, :from_singleton]
["#<Class:Outer>", "Outer"]
:name_error
[:inner, :b_const, :a_const]
["#<Class:A::B>", "A::B", "A"]
[]
["Plain"]
["Plain"]
:singleton_const
[false, true]
