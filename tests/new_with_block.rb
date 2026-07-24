# A literal block passed to `.new` is forwarded to `initialize`, so
# `yield`/`block_given?` inside it see the block.

class Builder
  attr_reader :items
  def initialize
    @items = []
    yield self if block_given?
  end
  def add(x) = @items << x
end

# The block runs during construction and captures an outer local.
label = "n"
b = Builder.new do |bld|
  3.times { |i| bld.add("#{label}#{i}") }
end
p b.items

# No block: block_given? is false.
p Builder.new.items

# Struct with a custom initialize can `super` into the member setter and yield.
Point = Struct.new(:x, :y) do
  def initialize(x, y)
    super(x * 2, y * 2)
    yield self if block_given?
  end
end
pt = Point.new(3, 4) { |p| puts "built (#{p.x}, #{p.y})" }
p [pt.x, pt.y]

# Data.define custom initialize forwarding through super.
Measure = Data.define(:n) do
  def initialize(n:)
    super(n: n * 10)
  end
end
p Measure.new(n: 4).n
