# `[]=` is an ordinary method, so an indexed assignment target carries
# however many indexes the receiver's `[]=` takes -- in compound
# assignment, in multi-assignment, and through a splat, each index
# expression evaluated exactly once.
#
# zeo's multi-assignment arm demanded a single index, and a SPLAT index in
# compound assignment fell off the end of the lowering (red-chainer's
# `gx[*i] += dot / (2 * eps)`).
class Grid
  def initialize
    @cells = {}
  end

  def [](*coords)
    @cells[coords] || 0
  end

  def []=(*coords, value)
    @cells[coords] = value
  end
end

g = Grid.new

# Multi-assignment with a two-argument index target.
g[1, 2], g[3, 4] = "a", "b"
puts g[1, 2]
puts g[3, 4]

# Compound assignment through a splat index, the coordinate array built
# (and its builder run) exactly once.
built = 0
coords = lambda do
  built += 1
  [5, 6]
end
g[*coords.call] += 10
g[*[5, 6]] += 1
puts g[5, 6]
puts built

# A splat index in multi-assignment position.
g[*[7, 8]], g[9] = "x", "y"
puts g[7, 8]
puts g[9]
__END__
a
b
11
1
x
y
