class B
  def initialize
    @a = []
    nil
  end

  def add(x)
    @a = @a + [x]
    nil
  end

  def get
    @a
  end
end

b = B.new
b.add("z")
p b.get

m = B.new
m.add("x")
m.add("y")
m.add("z")
p m.get
p m.get.length
p m.get.join(",")

class S
  def initialize
    @a = []
    nil
  end

  def add(x)
    @a = @a + [x]
    nil
  end

  def get
    @a
  end
end

[:z, 1.5, true, 1].each do |v|
  s = S.new
  s.add(v)
  p s.get
end

# the desugared and mutating spellings already worked; keep them pinned
class T
  def initialize
    @a = []
    nil
  end

  def plus(x)
    @a += [x]
    nil
  end

  def shovel(x)
    @a << x
    nil
  end

  def get
    @a
  end
end

t = T.new
t.plus("p")
t.shovel("s")
p t.get

# computed inside the class, which was correct before too
class U
  def initialize
    @a = []
    nil
  end

  def add(x)
    @a = @a + [x]
    nil
  end

  def joined
    @a.join(",")
  end

  def sorted
    @a.sort.join(",")
  end
end

u = U.new
u.add("b")
u.add("a")
p u.joined
p u.sorted
