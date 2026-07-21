# Array#select! / #filter! / #keep_if / #reject! / #delete_if filter the
# array in place and return the receiver. Elements keep their type.
a = [1, 2, 3, 4, 5]
a.select! { |x| x.even? }
p a

b = [1, 2, 3, 4, 5]
b.reject! { |x| x.even? }
p b

c = [1, 2, 3, 4, 5]
c.keep_if { |x| x > 2 }
p c

d = [1, 2, 3, 4, 5]
d.delete_if { |x| x > 2 }
p d

e = ["apple", "fig", "cherry"]
e.select! { |s| s.length > 3 }
p e

g = [1, "two", 3, "four"]
g.select! { |x| x.is_a?(Integer) }
p g

h = [:a, :bb, :ccc]
h.reject! { |x| x.size > 1 }
p h

# Return value is the (mutated) receiver when elements change.
i = [1, 2, 3, 4]
r = i.select! { |x| x > 2 }
p r
