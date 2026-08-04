# `t = Array.new(n) { Array.new(m, 0) }` is a table of int arrays, but the
# narrowing pass that gives an array of int-arrays its typed representation
# only classified pushes and literals -- a generator block killed the slot, so
# the table stayed a boxed poly array and every read of it went through
# sp_poly_arr_get. Reading the block's value as element evidence narrows it,
# and the generator emits the pointer array the slot now holds.
t = Array.new(3) { Array.new(4, 0) }
p t.length
p t[0].length
t[1][2] = 5
p t[1][2]
row = t[1]
p row[2]
p row.length
t[0][0] = t[0][0] + 7
p t[0][0]
s = 0
r = 0
while r < 3
  q = t[r]
  c = 0
  while c < 4
    s = s * 31 + q[c]
    c += 1
  end
  r += 1
end
p s
u = Array.new(2) { |i| Array.new(2) { |j| i * 2 + j } }
p u[1][1]
p u.map { |x| x.length }
# a generator whose element is NOT an int array keeps the boxed representation
v = Array.new(2) { |i| i.to_s }
p v
w = Array.new(2) { [1, "s"] }
p w[0][1]
