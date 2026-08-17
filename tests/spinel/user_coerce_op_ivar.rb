class Vec
  attr_reader :x
  def initialize(x); @x = x; end
  def +(o); Vec.new(x + o.x); end
  def bump(v); @x += v; self; end
  def coerce(n); [Vec.new(n), self]; end
end
p((Vec.new(1) + Vec.new(2)).x)
p(Vec.new(1).bump(5).x)

class Vec3
  attr_reader :x
  def initialize(x); @x = x; end
  def +(o); Vec3.new(x + o.x); end
  def *(n); Vec3.new(x * n); end
  def <<(v); @x += v; self; end
  def to_proc; ->(n) { n * @x }; end
end
p((Vec3.new(1) + Vec3.new(2)).x)
p((Vec3.new(1) << 2).x)

class Holder
  def initialize; @v = nil; end
  def set(o); @v = o; self; end
  def add(o); @v += o; self; end
  def v; @v; end
end
class Money
  attr_reader :c
  def initialize(c); @c = c; end
  def +(o); Money.new(c + o.c); end
  def to_s; "$#{@c}"; end
end
p Holder.new.set(2).add(3).v
p Holder.new.set(Money.new(2)).add(Money.new(3)).v.to_s
