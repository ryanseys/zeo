class Counter
  def initialize
    @n = 0
  end
  def n
    @n
  end
  def step
    @n = @n + 1
  end
  def under?(limit)
    n < limit
  end
end
c = Counter.new
c.step while c.under?(5)
puts c.n
