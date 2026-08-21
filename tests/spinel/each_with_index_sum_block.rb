# A sum over an each_with_index enumerator folds [value, index] pairs, so a
# 2-parameter block auto-splats each pair. The scalar-accumulator fold bound
# only the first parameter and left the rest nil, so `a[i]` read index nil on
# every iteration; and a parameter nothing reads had no slot at all, which
# named an undeclared identifier in the generated C.
p [0].each_with_index.sum { |e, i| 0 }
p [0].each_with_index.sum { |e| 0 }

a = [10, 20, 30]
p a.each_with_index.sum { |v, i| a[i] }
p a.each_with_index.sum { |v, i| v }
p a.each_with_index.sum { |v, i| i }
p a.each_with_index.sum { |v, i| v + i }

b = [1.5, 2.5]
p b.each_with_index.sum { |v, i| v * i }

# the shape the report came from: a ring of structs indexed by the neighbour
st = Struct.new(:x, :y)
ring = [[-63.1, 46.2], [-63.2, 46.3]].map { |lon, lat| st.new(Float(lon), Float(lat)) }
p ring.each_with_index.sum { |v, i|
  w = ring[i + 1 - ring.size]
  v.x * w.y - w.x * v.y
}.round(6)

# a plain array keeps binding the whole element to the first parameter
p [[1, 2], [3, 4]].sum { |pair| pair[0] }
p [1, 2].sum { |x| x }
