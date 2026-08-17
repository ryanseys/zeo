def fill(str, n)
  n.times { |i| str << "ab" << i.to_s }
  str
end

s = String.new
fill(s, 5)
p s
p s.length

# an ivar filled through a helper that takes it as a parameter
class Doc
  def initialize
    @buf = String.new
  end

  def add(n)
    n.times { |i| append(@buf, i) }
  end

  def append(str, i)
    str << "<" << i.to_s << ">"
  end

  def buf
    @buf
  end
end

d = Doc.new
d.add(4)
p d.buf
p d.buf.bytesize

# a frozen receiver still raises
f = "abc".freeze
begin
  f << "d"
  p :no_raise
rescue FrozenError => e
  p e.class
end

# the value read out before the append is not disturbed by a later grow
a = String.new
a << "one"
b = a.dup
a << "-two"
p [a, b]

# binary content survives
bin = String.new
bin << "x" << 0.chr << "y"
p bin.bytes
p bin.bytesize
