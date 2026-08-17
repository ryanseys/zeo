# Reading a mutable String out of a container into a local aliases the
# element: an in-place mutation through the local must show in the container,
# and `equal?` must answer true. The local used to hold a COPY (#3941).
rows = [+"abc"]
r = rows[0]
r.upcase!
p [rows, r, r.equal?(rows[0])]
r << "Z"
p rows
r.sub!("A", "q")
p rows
r.replace("new")
p rows

h = { a: +"abc" }
v = h[:a]
v << "Z"
p [h, v.equal?(h[:a])]
v.upcase!
p h

n = { 1 => +"one" }
w = n[1]
w << "!"
p n

# unrelated: a copy is still a copy when nothing mutates
plain = ["abc"]
q = plain[0]
p [q, plain]

# a store then an alias

store = {}
store[:k] = +"v"
sv = store[:k]
sv << "2"
p store
