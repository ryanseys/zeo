# User-defined << / >> dispatch to the class method, not a raw C shift.
class Buf
  def initialize
    @s = ""
  end
  def <<(x)
    @s = @s + x
    self
  end
  def >>(x)
    @s = x + @s
    self
  end
  def to_s
    @s
  end
end
b = Buf.new
b << "hi"
b << "!"
b >> "[" 
puts b.to_s
# Builtin << still works (regression guard).
a = []
a << 1
a << 2
puts a.length
s = "x"
s << "y"
puts s
