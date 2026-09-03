# A real, previously-undetected bug: `safe_ident` had no escaping at
# all for a method name that's a bare operator symbol (`+`, `<=>`,
# `[]`, ...) -- `def +(other)` panicked at CODEGEN time with a raw
# `proc_macro2` "not a valid Ident" error. This meant user-defined
# operator overloading -- claimed to "work for free"
# once operators became ordinary Calls -- never actually worked for
# the DEFINING side; every existing operator test exercised only
# native `Int`/`Float` fast paths, which bypass `safe_ident` entirely.
# Fixed via a fixed lookup table (`OPERATOR_METHOD_NAMES`) mapping
# every operator symbol to a valid Rust identifier, consulted by BOTH
# the method-definition side and the general-dispatch call site (the
# same function backs both), so they agree automatically.

class Vector
  attr_reader :x, :y
  def initialize(x, y)
    @x = x
    @y = y
  end
  def +(other)
    Vector.new(@x + other.x, @y + other.y)
  end
  def <=>(other)
    (@x * @x + @y * @y) <=> (other.x * other.x + other.y * other.y)
  end
  def to_s
    "(#{@x}, #{@y})"
  end
end
v1 = Vector.new(1, 2)
v2 = Vector.new(3, 4)
v3 = v1 + v2
puts v3.to_s
puts(v1 <=> v2)
puts(v2 <=> v1)
puts(v1 <=> v1)
__END__
(4, 6)
-1
1
0
