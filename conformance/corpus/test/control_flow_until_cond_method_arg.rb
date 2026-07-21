# Same re-evaluation requirement for an until condition argument.
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
  def reached?(v)
    v >= 3
  end
end
g = Gen.new
count = 0
until g.reached?(g.cur)
  g.advance
  count = count + 1
  break if count >= 100
end
puts count
