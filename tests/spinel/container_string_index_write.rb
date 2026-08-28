# `outer[i][k] = v` where the inner value is a String: spinel splices a String
# into a FRESH buffer, so the result has to be stored back into outer's slot.
# The store-back form existed for a statically Integer index and not for a boxed
# one, so an index that came out of a container -- a destructured block parameter
# whose receiver is a local, an element read -- wrote to the inner value alone
# and the assignment was silently dropped (#4067).
pairs = [[0, 1]]

g = ["abc", "def"].map(&:dup)
pairs.each { |r, c| g[0][c] = "*" }
p g

h = { 0 => +"abc" }
pairs.each { |r, c| h[0][c] = "*" }
p h

# a single (non-destructured) boxed index, and one read out of an array
idx = [[1]].first
s = ["abcd"].map(&:dup)
s[0][idx.first] = "1"
p s

# nesting an Array rather than a String still mutates in place
t = [[1, 2], [3, 4]]
pairs.each { |r, c| t[0][c] = 9 }
p t

# a nested Hash inner, keyed by the boxed index
u = [{ 1 => "x" }]
pairs.each { |r, c| u[0][c] = "y" }
p u

# a negative index counts from the end, and one past the end appends
v = ["abc"].map(&:dup)
[[-1]].each { |(n)| v[0][n] = "Z" }
p v
w = ["ab"].map(&:dup)
[[2]].each { |(n)| w[0][n] = "c" }
p w

# out of range raises IndexError naming the original index
x = ["ab"].map(&:dup)
begin
  [[9]].each { |(n)| x[0][n] = "!" }
rescue IndexError => e
  p e.message
end
