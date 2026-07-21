# method(:accessor) for auto-generated attr / struct accessors (no real def):
# reader, writer, name, arity, and to_proc with a correct-arity block.
class C
  attr_accessor :x, :name
  def initialize; @x = 7; @name = "hi"; end
end
o = C.new
p o.method(:x).call
p o.method(:name).call
m = o.method(:x=)
m.call(9)
p o.x
p o.method(:x).name
p o.method(:x).arity
p o.method(:x=).arity
[1, 2, 3].each(&o.method(:x=))
p o.x

S = Struct.new(:ok?, :count)
s = S.new(true, 0)
p s.method(:ok?).call
s.method(:"ok?=").call(false)
p s.ok?
p s.method(:ok?).arity
p s.method(:"ok?=").arity

# a real def is still resolved (regression guard)
class D
  def double(n); n * 2; end
end
p D.new.method(:double).call(21)
