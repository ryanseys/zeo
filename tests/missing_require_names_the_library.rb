require "stringio"

io = StringIO.new
io << "ok"
p io.string

class Set
  def initialize
    @a = []
  end

  def add(x)
    @a << x
    self
  end

  def size
    @a.size
  end
end

s = Set.new
s.add(1).add(2)
p s.size
