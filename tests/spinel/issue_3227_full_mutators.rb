# Shared-mutable strings: every in-place mutator shares through aliases and
# containers (P1+P2 of the reference-semantics plan).
s = "hello world"
arr = [s]
s << "!"
s["world"] = "ruby"
p arr[0]
s.insert(0, ">> ")
p arr[0]
p s.slice!(0, 3)
p arr[0]
s.clear
p arr[0]
p arr[0].equal?(s)

# bang-only alias set
b1 = "low"
b2 = b1
b1.upcase!
p b2
# mixed alias set
c1 = "a"
c2 = c1
c1 << "b"
c1.upcase!
p c2
# replace via alias
d1 = "abc"
d2 = d1
d1.replace("xyz")
p d2
# non-literal write into a shared set
def make = "dyn" + "amic"
e1 = make
e2 = e1
e1 << "!"
p e2
p e1.equal?(e2)
# frozen source stays frozen through the handle
f1 = make.freeze
f2 = f1
begin
  f1 << "x"
rescue FrozenError
  p :frozen_ok
end
p f2
# setbyte shares
g1 = "Abc"
g2 = g1
g1.setbyte(0, 97)
p g2
