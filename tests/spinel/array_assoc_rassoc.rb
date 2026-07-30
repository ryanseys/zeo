# Array#assoc / #rassoc match the first / second element of each sub-array.
# A like-typed pair such as [1, 2] is an IntArray (not a PolyArray), so the
# match must compare and return it by dispatching on the pair's own kind.
p [[1, 2], [3, 4]].assoc(3)
p [[1, 2], [3, 4]].rassoc(2)
p [[1, 2], [3, 4]].assoc(9)
p [[1, 2], [3, 4]].rassoc(9)

# String/mixed pairs (PolyArray) and a non-literal receiver.
p [["a", 1], ["b", 2]].assoc("b")
p [["a", 1], ["b", 2]].rassoc(1)

def assoc_of(pairs, key)
  pairs.assoc(key)
end
p assoc_of([[10, 100], [20, 200]], 20)

# Symbol-array pairs match and round-trip through their own kind.
p [[:a, :b], [:c, :d]].assoc(:c)
p [[:a, :b], [:c, :d]].rassoc(:b)

# A nil key must not match an empty / too-short pair or a non-array element.
p [[], [1], (1..2)].assoc(nil)
p [[], [1], (1..2)].rassoc(nil)

# An element that IS the receiver. Both methods lock each sub-array to read
# its first / second slot, so walking the receiver under its own guard makes
# that a self-lock -- which hangs forever rather than raising. Snapshotting
# the receiver first is what keeps these terminating.
selfref = [1]
selfref << selfref
p selfref.assoc(1)
p selfref.rassoc(1)
p selfref.assoc(nil)

# The same shape one level down: the pair being probed contains the outer
# array, so the probe's own element read re-enters it.
outer = [[9, nil]]
outer[0][1] = outer
p outer.assoc(9)
p outer.rassoc(outer)
