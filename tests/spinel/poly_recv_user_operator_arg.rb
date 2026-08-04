# An operator reached through a poly receiver goes out to the runtime's
# user-binop dispatch, which hands the argument over boxed. Typed from the one
# call site the compiler could resolve -- the `1 / box` coercion, whose
# argument is a Box -- the dispatch arm was guarded on that class, and the same
# operator called with an Integer fell through to NoMethodError on a method the
# class defines (#3511).
class Box
  attr_reader :v
  def initialize(v); @v = v; end
  def self.scalar(x); new(x); end
  def coerce(other); [Box.scalar(other), self]; end
  def +(o); Box.new(@v + (o.is_a?(Box) ? o.v : o)); end
  def /(other)
    o = other.is_a?(Box) ? other : Box.scalar(other)
    Box.new(Rational(@v) / o.v)
  end
  def *(other)
    o = other.is_a?(Box) ? other : Box.scalar(other)
    Box.new(@v * o.v)
  end
  def to_s; "Box(#{@v})"; end
end

time = Box.new(2)
puts "1 / time = #{1 / time}"

items = [Box.new(3), Box.new(4)]
total = items.inject(Box.new(0)) { |acc, r| acc + r }
puts "mean = #{total / items.length}"
puts "twice = #{total * 2}"
puts "by box = #{total / Box.new(2)}"
puts "sum = #{total}"

# the same operator on a monomorphic receiver keeps working
direct = Box.new(9)
puts "direct = #{direct / 3}"
puts "direct box = #{direct / Box.new(3)}"

# and through the coercion in both directions
puts "2 * box = #{2 * Box.new(5)}"
puts "box * 2 = #{Box.new(5) * 2}"
