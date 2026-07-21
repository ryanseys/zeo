# proc/lambda returning an array built from a block param must carry the
# param's real type through .call, not leak it as a raw pointer-int (#1372).
f = proc { |t| [t] }
p f.call("x")
g = ->(t) { [t] }
p g.call("y")

class Wrap
  def initialize(t); @t = t; end
  def pair; [@t, @t]; end
end
h = proc { |text| Wrap.new(text).pair }
p h.call("z")

# arithmetic lambda still defaults its param to int
add = ->(n) { n + 1 }
puts add.call(5)
