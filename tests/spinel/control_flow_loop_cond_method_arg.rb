# A method call used as an ARGUMENT inside a while condition must be
# re-evaluated every iteration, not hoisted into a temp before the loop.
class Gen
  def initialize
    @x = 0
  end
  def cur
    @x
  end
  def advance
    @x = @x + 1
  end
  def small?(v)
    v < 3
  end
end
g = Gen.new
count = 0
while g.small?(g.cur)
  g.advance
  count = count + 1
  break if count >= 100
end
puts count
