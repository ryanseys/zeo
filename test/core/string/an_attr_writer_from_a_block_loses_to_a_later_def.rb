class C
  attr_writer :x
  def x=(v)
    @x = [v, v]
  end
  attr_reader :x
end
c = C.new
c.x = 1
p c.x

class D
  [:y].each { |a| attr_writer(a) }
  def y=(v)
    @y = [v, v]
  end
  attr_reader :y
end
d = D.new
d.y = 2
p d.y

class E
  %i[z].each { |a| attr_accessor(a) }
  def z = 9
end
p E.new.z

class F
  def self.make(n) = attr_writer(n)
  make :w
  def w=(v)
    @w = [v]
  end
  attr_reader :w
end
f = F.new
f.w = 3
p f.w
__END__
[1, 1]
[2, 2]
9
[3]
