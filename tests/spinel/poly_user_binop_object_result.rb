# A user operator with a POLY operand runs through the boxed arithmetic path,
# but the fixpoint still settles the call's type on the class that defines it.
# `(a % o).value` then read a field off an sp_RbVal.

class Int64
  attr_reader :value
  def initialize(v); @value = v; end
  def +(other)
    Int64.new(@value + other.value)
  end
  def %(other)
    b = other.value
    return Int64.new(0) if b == 0
    Int64.new(@value % b)
  end
end

class Wrap
  attr_reader :value
  def initialize(v); @value = v; end
end

def pick(f, n)
  f ? Int64.new(n) : Wrap.new(n)
end

a = Int64.new(17)
o = pick(true, 5)
p (a % o).value
p (a + o).value
