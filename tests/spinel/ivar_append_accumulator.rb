class Doc
  attr_reader :out

  def initialize
    @out = String.new
    @tail = String.new
  end

  def build(n)
    @out = String.new
    n.times { |i| @out << i.to_s << "," }
    @out
  end

  def mixed(v, n)
    @tail = v.upcase
    n.times { @tail << "!" }
    @tail
  end
end

d = Doc.new
p d.build(5)
p d.out
p d.out.length
p d.build(3)
p d.mixed("ab", 3)
p d.out == "0,1,2,"

# the accumulator is still an ordinary String to everyone else
p d.build(2)
p d.out.reverse
p d.out.sub("0", "X")
