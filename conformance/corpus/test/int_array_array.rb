# Array of int-arrays (TY_INT_ARRAY_ARRAY): a monomorphic array whose elements
# are themselves int arrays is stored unboxed (sp_PtrArray of sp_IntArray*),
# so indexing yields a typed int array with no per-element boxing.
module Field
  def self.mul(a, b) = (a * b) % 2013265921
  def self.add(a, b) = (a + b) % 2013265921
end
module Ext
  def self.scale(a, s)
    [Field.mul(a[0], s), Field.mul(a[1], s), Field.mul(a[2], s), Field.mul(a[3], s)]
  end
  def self.add(a, b)
    [Field.add(a[0], b[0]), Field.add(a[1], b[1]), Field.add(a[2], b[2]), Field.add(a[3], b[3])]
  end
end

alphas = []
8.times { |i| alphas << [i + 1, i + 2, i + 3, i + 4] }

acc = [0, 0, 0, 0]
1000.times do |t|
  c = t % 8
  acc = Ext.add(acc, Ext.scale(alphas[c], t))
end
p acc

# container ops: index, first, last, length, []=
p alphas.length
p alphas[0]
p alphas.first
p alphas.last
alphas[2] = [99, 98, 97, 96]
p alphas[2]

# element pulled into a local, then indexed
row = alphas[3]
p row[1]

# push more, iterate by index
alphas << [10, 20, 30, 40]
sum = 0
alphas.length.times { |k| sum += alphas[k][0] }
p sum

# array-of-int-array LITERAL narrows the same way a pushed one does (the
# OA_CLS_IA sentinel is negative but IS evidence, not an invalid marker)
rows = [[1, 2], [3, 4], [5, 6]]
p rows.length
p rows[0]
p rows[2]
tot = 0
rows.length.times { |k| tot += rows[k][0] + rows[k][1] }
p tot
