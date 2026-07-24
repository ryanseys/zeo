# A `super` inside a method defined in a NON-const `Struct.new` / `Data.define`
# block, or a `Class.new` block, resolves at runtime (#192): the class is minted
# at runtime, so there is no compile-time defining class to splice -- `super`
# reads the class off the runtime method-frame stack instead.

# explicit positional super into the native Struct member-binding initialize
pair = Struct.new(:x, :y) do
  def initialize(x)
    super(x, x * 2)
  end
end
p pair.new(5).to_a

# bare (zsuper) forwarding of the method's own params
echo = Struct.new(:a, :b) do
  def initialize(a, b)
    super
  end
end
p echo.new(1, 2).to_a

# keyword super into the native Data initialize
coord = Data.define(:lat, :lng) do
  def initialize(lat:, lng:)
    super(lat: lat * 10, lng: lng)
  end
end
p coord.new(lat: 1, lng: 2)

# super from a Class.new override into a runtime parent's method
base = Class.new do
  def greet(n)
    "hi #{n}"
  end
end
sub = Class.new(base) do
  def greet(n)
    super(n) + "!"
  end
end
p sub.new.greet("y")
