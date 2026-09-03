# `Class#allocate` builds an instance WITHOUT running `initialize` (so a
# required-arg `initialize` is bypassed); the object is still a usable
# instance whose ivars start nil and can be assigned by hand. A builtin
# value class allocates its empty value, and a variable-held class
# dispatches like the constant.

class Thing
  def initialize(x) = (@x = x)
  def x = @x
end
t = Thing.allocate
p t.class.name
p t.x
t2 = Thing.allocate
p t2.is_a?(Thing)
p String.allocate
p Array.allocate
p Hash.allocate
w = Thing
p w.allocate.class
__END__
"Thing"
nil
true
""
[]
{}
Thing
