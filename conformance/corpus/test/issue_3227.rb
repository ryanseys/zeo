# In-place mutation through a local alias is observed (shared String object).
s1 = "hello"
s2 = s1
p s1.equal?(s2)
s1 << " world"
p s1
p s2
p s1.equal?(s2)

# chained aliases share the one object
a = "x"
b = a
c = b
a << "y"
p [a, b, c]
p a.equal?(c)

# a string that is aliased but never mutated is unaffected
m = "keep"
n = m
p n
p m.equal?(n)
