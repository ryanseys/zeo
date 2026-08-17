# An array pattern over an object binds what its #deconstruct returns: the
# elements used to default to Integer, so a Symbol element bound its boxed bits
# as a small int (#3954).
class Sym
  def deconstruct = [:x, :y]
end
case Sym.new
in [a, b] then p [a, b]
end

class Mixed
  def deconstruct = ["s", 1]
end
case Mixed.new
in [ma, mb] then p [ma, mb]
end

class Ints
  def deconstruct = [1, 2, 3]
end
case Ints.new
in [ia, *irest] then p [ia, irest]
end

class Strs
  def deconstruct = %w[p q r]
end
case Strs.new
in [sa, *srest] then p [sa, srest]
end

class Node
  def initialize(k, v)
    @k = k
    @v = v
  end
  def deconstruct = [@k, @v]
end
case Node.new(:binop, 7)
in [k, v] then p [k, v]
end

# Two arms whose bindings share a name over different array kinds: the rest
# slice takes the boxed form the required bindings already used.
class I2; def deconstruct = [1, 2, 3]; end
class S2; def deconstruct = %w[p q r]; end
case I2.new
in [h, *t] then p [h, t]
end
case S2.new
in [h, *t] then p [h, t]
end
