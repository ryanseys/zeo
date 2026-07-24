# Shared-mutable strings: container-held aliases observe the in-place
# transforming mutators, not just <<.
m = "low"
box = [m]
m.upcase!
p box[0]

s = "a"
arr = [s]
s << "b"
s.upcase!
p arr[0]

t = "hello"
h = { k: t }
t.gsub!("l", "L")
p h[:k]

r = "abc"
c = [r]
r.replace("xyz")
p c[0]

q = "tail"
d = [q]
q.prepend("head-")
p d[0]

u = "abc"
e = [u]
u.reverse!
p e[0]
p e[0].equal?(u)

w = "  padded  "
f = [w]
w.strip!
p f[0]

x = "prefix-body"
g = [x]
x.delete_prefix!("prefix-")
p g[0]
