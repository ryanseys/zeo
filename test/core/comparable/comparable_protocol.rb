# The comparison protocol: a user `<=>` drives Comparable, and its result is
# validated the way CRuby's `rb_cmpint` does.

# `<=>` may return any numeric type; only its SIGN matters, so a Float result
# works just like an Integer one.
class Temp
  include Comparable
  attr_reader :deg
  def initialize(deg) = @deg = deg
  def <=>(other) = (deg - other.deg).to_f
end

p [Temp.new(3), Temp.new(1), Temp.new(2)].sort.map(&:deg)   # [1, 2, 3]
p Temp.new(5) < Temp.new(9)                                 # true
p Temp.new(5).clamp(Temp.new(1), Temp.new(9)).deg           # 5
p Temp.new(5) == Temp.new(5)                                # true

# A `<=>` that answers `nil` (incomparable) makes ordering raise, but `==`
# stays false and identity always wins.
class Ver
  include Comparable
  def initialize(n) = @n = n
  attr_reader :n
  def <=>(other) = other.is_a?(Ver) ? (n <=> other.n) : nil
end
a = Ver.new(1)
p a == Ver.new(2)                                  # false
p a == a                                           # true (identity)
p(begin; a < "x"; rescue => e; e.class; end)       # ArgumentError

# clamp: nil bounds are open on that side; reversed bounds raise; an exclusive
# range cannot clamp.
p 5.clamp(1, nil)                                            # 5
p 5.clamp(nil, 3)                                            # 3
p(begin; 5.clamp(10, 1); rescue => e; e.message; end)       # min argument ...
p 5.clamp(1..10)                                            # 5
p(begin; 5.clamp(1...10); rescue => e; e.message; end)      # cannot clamp ...

# min/max/sort over mutually incomparable elements raise ArgumentError (built
# from a variable so the array stays heterogeneous).
x = "a"
p(begin; [1, x, 2].min; rescue => e; e.message; end)        # String with 1
p(begin; [1, x].sort; rescue => e; e.class; end)            # ArgumentError

# Hash#sort orders the [key, value] pairs through the array comparison.
p({ b: 2, a: 1, c: 3 }.sort)                  # [[:a, 1], [:b, 2], [:c, 3]]
__END__
[1, 2, 3]
true
5
true
false
true
ArgumentError
5
3
"min argument must be less than or equal to max argument"
5
"cannot clamp with an exclusive range"
"comparison of String with 1 failed"
ArgumentError
[[:a, 1], [:b, 2], [:c, 3]]
