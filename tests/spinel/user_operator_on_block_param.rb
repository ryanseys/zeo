# A user object reaching an operator through a block parameter is a BOXED
# receiver, and only `+`, `-` and `*` found their way to the class's own
# method: every other operator coerced the object to an integer, the
# comparisons failed outright, and `<<` was read as an array push that typed
# the loop variable an int array (#3501, #3502). The same expressions on the
# same object held in a plain local were always correct, which is what makes
# these worth pinning in both positions.
class Money
  include Comparable
  attr_reader :cents

  def initialize(cents)
    @cents = cents
  end

  def /(n)
    Money.new(@cents / n)
  end

  def %(n)
    Money.new(@cents % n)
  end

  def **(n)
    Money.new(@cents**n)
  end

  def &(n)
    Money.new(@cents & n)
  end

  def |(n)
    Money.new(@cents | n)
  end

  def ^(n)
    Money.new(@cents ^ n)
  end

  def <<(n)
    Money.new(@cents << n)
  end

  def >>(n)
    Money.new(@cents >> n)
  end

  def +(n)
    Money.new(@cents + n)
  end

  def <=>(other)
    @cents <=> other.cents
  end

  def to_s
    "$#{@cents}"
  end
end

[Money.new(250)].each do |m|
  puts (m / 5).to_s
  puts (m % 7).to_s
  puts (m + 1).to_s
  puts (m & 12).to_s
  puts (m | 1).to_s
  puts (m ^ 3).to_s
  puts (m << 2).to_s
  puts (m >> 1).to_s
  puts (m < Money.new(300))
  puts (m > Money.new(300))
  puts (m <=> Money.new(250))
  puts (m == Money.new(250))
end

[Money.new(3)].each { |m| puts (m**2).to_s }

# the same object in a plain local still behaves
m = Money.new(250)
puts (m / 5).to_s
puts (m << 2).to_s
puts (m < Money.new(300))
puts (m <=> Money.new(250))

# a genuine push accumulator keeps its promotion even though a class owns `<<`
out = []
[1, 2, 3].each { |x| out << x * 2 }
p out
