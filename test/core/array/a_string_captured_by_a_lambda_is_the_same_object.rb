# Mutating it inside the lambda is visible outside, and rebinding is not.
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
__END__
"cap!"
true
"two!"
"one"
"held.."
"px"
"p"
