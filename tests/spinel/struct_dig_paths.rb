# dig through and into a Struct.
Leaf = Struct.new(:v)
Box = Struct.new(:label, :items)

# a Struct member holding an array: the member's array kind had no C name for
# a poly array, and the NULL went straight into the emitted symbol
b = Box.new("x", [Leaf.new(2)])
p b.dig(1, 0)
p b.dig(:items, 0)
p b.dig(0)
p b.dig(:label)

# a Struct read out of a container is diggable, by member name or by offset;
# it was refused, and a String or Symbol key reached the offset slot as a
# pointer
a = [Leaf.new(2)]
p a.dig(0)
p a.dig(0, "v")
p a.dig(0, :v)
p a.dig(0, 0)
p a.dig(1, :v)

h = { k: Leaf.new(5) }
p h.dig(:k, :v)

# nested structs still resolve at compile time
Inner = Struct.new(:c)
Outer = Struct.new(:b)
p Outer.new(Inner.new(7)).dig(:b, :c)

# a scalar intermediate is still a TypeError
p(([[1]].dig(0, 0, 0) rescue $!.class))
