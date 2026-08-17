# A user-defined #! on an object with ivars: the receiver is passed the way
# its layout says, by value for a value type, and the cast to a pointer did
# not even compile (#3819).
class N1
  def initialize(b); @b = b; end
  def !; @b == 0; end
end
p(!N1.new(1))
p(!N1.new(0))
x = N1.new(0)
p(!x)
p(!!x)

class N2
  def initialize(b); @b = b; end
  def !=(o); @b != o; end
end
p(N2.new(3) != 3)
p(N2.new(3) != 4)
