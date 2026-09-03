# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A Syn holds its source Neu and that Neu's `syns_out` holds the Syn back.
#@ gccheck: cycle leak: 10 objects (Array x4, Syn x4, Neu x2)
class Syn
  attr_accessor :weight, :src
  def initialize(src); @src = src; @weight = 0.5; end
end
class Neu
  attr_accessor :syns_in, :syns_out, :output
  def initialize; @syns_in = []; @syns_out = []; @output = 1.0; end
end
class Net
  def initialize(n)
    @a = (1..n).map { Neu.new }
    @b = (1..n).map { Neu.new }
    @a.each do |src|
      @b.each do |dst|
        s = Syn.new(src)
        src.syns_out << s
        dst.syns_in << s
      end
    end
  end
  def upd(rate)
    @b.each do |nu|
      nu.syns_in.each do |s|
        s.weight += rate * 0.1 * s.src.output
      end
    end
  end
  def show
    p @b[0].syns_in.length
    p @b[0].syns_in[0].weight.round(3)
    p @a[0].syns_out[0].src.output
  end
end
n = Net.new(2)
n.upd(1.0)
n.show

class Bag
  attr_reader :items
  def initialize; @items = []; end
end
b = Bag.new
b.items << "x"
b.items << "y"
p b.items.length
p b.items[0].upcase
p b.items.join("-")
__END__
2
0.6
1.0
2
"X"
"x-y"
