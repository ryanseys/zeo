# The bucket index takes the low bits of a key's hash, and several key kinds
# leave their information out of those (a Float's mantissa bits are zero for a
# whole number, an array of small coordinates folds to h*31+x). Those keys
# landed on a handful of slots however large the table grew, and lookups
# degenerated into linear scans. This only shows as time, so what the test
# holds is that the entries are all there and answer correctly.
n = 4000
h = {}
i = 0
while i < n
  h[[i % 40, i / 40]] = i
  i += 1
end
p [h.size, h[[7, 3]], h[[39, 99]], h[[40, 0]]]

fh = {}
i = 0
while i < n
  fh[i.to_f] = i
  i += 1
end
p [fh.size, fh[7.0], fh[3999.0], fh[4000.0]]

st = Struct.new :a, :b
sh = {}
i = 0
while i < n
  sh[st.new(i % 40, i / 40)] = i
  i += 1
end
p [sh.size, sh[st.new(7, 3)], sh[st.new(39, 99)]]

rh = {}
i = 0
while i < 500
  rh[(i..i + 1)] = i
  i += 1
end
p [rh.size, rh[(7..8)], rh[(499..500)]]

g = (0...n).group_by { |v| [v % 40, v / 40] }
p [g.size, g[[7, 3]], g[[0, 0]].size]

# the same keys still compare and enumerate in insertion order
p h.keys.first(3)
p h.to_a.last
