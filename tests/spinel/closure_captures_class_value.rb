# A class held in a local rides a cell when a closure captures it, the way a
# float does: a cls_id and a rodata name, with no GC pointer of its own. It had
# no cell shape, so the capture was refused outright -- reachable as soon as the
# receiver is boxed and the block is a real closure rather than an inlined loop.
# The capture struct also names its fields after the Ruby locals, so a local
# named for a C keyword (`struct`, `int`, `default`) has to be prefixed.
rings = ""
rings = [[1, 2], [3, 4]]

struct = Struct.new :x, :y
int = 5
default = "d"

# the reported shape: the block never runs here, but it still has to compile
empty = ""
empty = []
empty.each do |ring|
  ring.sum do |lon, lat|
    struct.new lon, lat
  end
end

p rings.map { |r| struct.new(r[0], r[1]).x + int }
p rings.map { |_r| default }
p rings.map { |r| struct.new(r[0], r[1]).to_a }

mk = struct
p mk.new(9, 8).y

k = String
p rings.map { |_r| k }.first
puts "OK"
