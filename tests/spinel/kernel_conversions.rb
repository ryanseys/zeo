# Kernel#Hash / #warn / #printf / #putc / p() value position (#2513, #2526, #2541)
p(Hash(nil))
p(Hash([]))
p(Hash({a: 1}))
begin; Hash([1, 2]); rescue => e; p e.class; end
w = warn("to stderr")
p(w)
f = printf("%d-%s\n", 7, "x")
p(f)
y = p()
p(y)
z = putc(65); puts
p(z)
