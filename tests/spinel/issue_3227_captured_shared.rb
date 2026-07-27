# Shared-mutable strings captured by closures (P6): the handle rides a
# typed-pointer cell, so capture+mutation and capture+rebinding both share.
s = +"cap"
t = s
cb = -> { s << "!" }
cb.call
p t
p s.equal?(t)

u = "one"
v = u
rb = -> { u = +"two"; u << "!" }
rb.call
p u
p v

procs = []
w = +"held"
x = w
procs << -> { w << "." }
2.times { procs[0].call }
p x

plain = "p"
pp2 = -> { plain + "x" }
p pp2.call
p plain
