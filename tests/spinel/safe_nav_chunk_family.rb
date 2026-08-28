# `v&.chunk_while { }` on a nil receiver answers nil. The lowering that builds
# the runs walked the receiver without looking at the operator, and the type of
# the call disagreed with what that lowering renders.
def pick(n) = n > 0 ? [1, 2, 4, 5] : nil

[1, 0].each do |k|
  v = pick(k)
  cw = v&.chunk_while { |a, b| b == a + 1 }
  sw = v&.slice_when { |a, b| b != a + 1 }
  p cw.nil?
  p sw.nil?
  p(cw.nil? ? nil : cw.to_a)
  p(sw.nil? ? nil : sw.to_a)
end

# the same call with a `.to_a` terminal answers the runs, not an Enumerator,
# whether the receiver is typed or poly
def runs(v) = v.chunk_while { |a, b| b == a + 1 }.to_a
p runs([1, 2, 4, 5])
p runs(pick(1))
