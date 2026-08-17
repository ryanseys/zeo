# A bignum operand must survive the evaluation of the other side, which can
# run arbitrary code (and collect).
class Run
  def initialize
    @acc = 0
  end

  def work(n)
    # allocate enough to force collections while the other operand waits
    junk = []
    n.times { |i| junk << "s#{i}" * 3 }
    v = 0
    n.times { |i| v = ((v << 2) + (i % 7)) & 0xFFFFFFFFFFFFFFFF }
    v
  end

  def step(n)
    @acc = (@acc + work(n).to_i) & 0xFFFFFFFF
  end

  def acc
    @acc
  end
end

r = Run.new
5.times { r.step(3000) }
p r.acc
p r.acc & 0xFFFF
