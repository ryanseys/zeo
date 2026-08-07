# A class/module DEFINITION binds its name in the immediately enclosing scope.
# Ruby never searches outward for it, so a `NAME = SomeClass` alias written in
# another scope is a different constant and cannot be what the definition
# reopens. zeo used to consult a whole-program scan of every `NAME = ...` here,
# unscoped, first match wins -- which bound these to the alias instead.

# 1. An alias in an OUTER scope, to a class, vs a nested MODULE of that name.
module Mongoid
  class Boolean; end
  module Fields
    Boolean = Mongoid::Boolean
  end
  module Extensions
    module Boolean
      def self.evolve(v) = !!v
    end
  end
end
p Mongoid::Boolean.class
p Mongoid::Extensions::Boolean.class
p Mongoid::Boolean.equal?(Mongoid::Extensions::Boolean)
p Mongoid::Extensions::Boolean.evolve(1)

# 2. A TOP-LEVEL module of the same name vs a nested class.
module HTML; end
class Thor
  module Shell
    class Basic
      def kind = :basic
    end
    class HTML < Basic
      def kind = :html
    end
  end
end
p Thor::Shell::HTML.superclass
p Thor::Shell::HTML.new.kind
p HTML.class

# 3. A top-level alias vs a nested class with a DIFFERENT superclass -- the
#    shape that reported `superclass mismatch for class ParseError`.
module Racc
  class ParseError < StandardError; end
end
ParseError = Racc::ParseError
class OptionParser
  class ParseError < RuntimeError; end
end
p ParseError.superclass
p OptionParser::ParseError.superclass
p ParseError.equal?(OptionParser::ParseError)

# 4. The alias reopen this machinery exists for still works: a bare TOP-LEVEL
#    definition whose name is a top-level alias reopens the aliased class.
module Tagged
  def tagged = :yes
end
INT_ALIAS = 1.class
class INT_ALIAS
  include Tagged
end
p 7.tagged
p INT_ALIAS.equal?(Integer)
