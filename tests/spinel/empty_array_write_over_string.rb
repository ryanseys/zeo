# An empty array literal carries no type of its own, so the union of a local's
# writes kept the other write's type and the array was emitted into a slot
# shaped for it: `a = ""; a = []` assigned an sp_IntArray to a const char *.
# A non-empty literal already widened the slot to the boxed one.
a = ""
a = []
a.each { |b| b }
p a

b = ""
b = [1]
p b

c = 1
c = []
p c

d = []
d = ""
p d

e = []
e << 1
p e

f = []
p f
p f.size

g = {}
g = []
p g
