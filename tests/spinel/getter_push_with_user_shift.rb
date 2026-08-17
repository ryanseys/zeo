# `dest.list << x` takes its element evidence through the getter's backing
# ivar. The push was declined outright whenever ANY user class defined `<<`
# (a bundled csv or a Set-like class is enough), so the ivar kept the empty
# literal's bottom kind and every element read back as an Integer.

class Collector
  def initialize
    @items = []
  end

  def <<(x)
    @items << x
    self
  end

  def size
    @items.size
  end
end

class Synapse
  attr_accessor :weight
  def initialize(w)
    @weight = w
  end
end

class Neuron
  attr_accessor :synapses_in

  def initialize
    @synapses_in = []
  end

  def bump(rate)
    synapses_in.each do |s|
      s.weight += rate
    end
  end

  def total
    synapses_in.reduce(0.0) { |acc, s| acc + s.weight }
  end
end

n = Neuron.new
[0.5, 1.5].each { |w| n.synapses_in << Synapse.new(w) }
n.bump(1.0)
p n.total

c = Collector.new
c << 1 << 2
p c.size
