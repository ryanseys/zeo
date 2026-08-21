# `["a"].map { |s| s.dup }` fills the array with fresh mutable strings exactly
# as `["a".dup]` does, and the shared-mutable machinery has to read the two
# shapes the same way. Only the literal was: the store analysis knew array and
# hash literals, push / << / []=, and not the array a collecting iterator
# builds. So an element of a mapped array kept the plain value representation,
# and mutating it was refused or silently dropped (#4037):
#
#   overlay = ["abc"].map(&:dup)
#   overlay[0][1] = "*"      # undefined method '[]=' for an instance of String
#   overlay[0] << "d"        # no error, mutation dropped
overlay = ["abc"].map(&:dup)
overlay[0][1] = "*"
p overlay

a = ["abc"].map { |s| s.dup }
a[0] << "d"
p a

b = ["abc"].map(&:dup)
s = b[0]
s[1] = "*"
p b
p b[0].respond_to?(:[]=)
p b[0].frozen?

c = ["abc"].map(&:dup)
c.each { |e| e << "!" }
p c

d = %w[a b].map(&:dup)
d[1] << "z"
p d

# a transforming map answers a fresh string too
e = ["abc"].map { |s| s.upcase }
e[0] << "d"
p e

# and the literal shapes that already worked
g = ["abc".dup]
g[0] << "d"
p g
h = [String.new("abc")]
h[0][1] = "*"
p h
