# `arr.each_with_index` with no block answers an Enumerator. A Struct gets an
# Enumerable #each_with_index synthesized, and a user definition of the name
# takes over the poly dispatch -- so the call typed as that Struct, and the
# Enumerator it actually builds was assigned to a slot declared as a pointer to
# the Struct (#4021).
#
# The receiver has to arrive boxed for the dispatch to be consulted at all,
# which is what the destructured multi-value return in the report does.
Line = Struct.new(:number)

def first_pass(lines)
  layout = []
  lines.each { |line| layout << [0, line] }
  [{}, layout, 0]
end

def encode(line) = line.number

symbols, layout, size = first_pass([Line.new(1), Line.new(2)])
image = layout.map { |_addr, line| encode(line) }
p image
reencoded = layout.each_with_index.map { |(_a, line), i| encode(line) == image[i] }
p reencoded

# the enumerator itself, and the other blockless producer
_s, lay2, _z = first_pass([Line.new(7)])
e = lay2.each_with_index
p e.class
p e.to_a.size
p lay2.each_index.to_a

# a literal array is unaffected, as it always was
lit = [[0, Line.new(3)]]
p lit.each_with_index.map { |(_a, line), i| line.number + i }
p lit.each_with_index.class
