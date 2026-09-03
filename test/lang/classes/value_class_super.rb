# A Struct or Data value class may define a custom `initialize` that calls
# `super` to set its members -- bare, explicit-positional, or (for Data)
# keyword super.

# Bare `super` forwards this method's own params into the members.
Trip = Struct.new(:x, :y, :z) do
  def initialize(x, y)
    super             # sets x and y; z stays nil
  end
end
p Trip.new(1, 2).to_a           # [1, 2, nil]

# Explicit positional super can compute extra members.
Vec = Struct.new(:a, :b, :sum) do
  def initialize(a, b)
    super(a, b, a + b)
  end
end
p Vec.new(3, 4).to_a            # [3, 4, 7]

# Data delegates by KEYWORD; omitting a member raises ArgumentError, and args
# routed through a method aren't constant-folded at the call site.
Scaled = Data.define(:x, :y) do
  def initialize(x:, y:)
    super(x: x * 100, y: y + 1)
  end
end
def make_scaled(a, b)
  Scaled.new(x: a, y: b)
end
p make_scaled(5, 2)             # #<data Scaled x=500, y=3>

Pair = Data.define(:m, :n) do
  def initialize(m:, n:)
    super(m: m)                 # forgets :n
  end
end
begin
  Pair.new(m: 1, n: 2)
rescue ArgumentError => e
  puts "error: #{e.message}"    # missing keyword: :n
end
__END__
[1, 2, nil]
[3, 4, 7]
#<data Scaled x=500, y=3>
error: missing keyword: :n
