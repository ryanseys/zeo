p(begin; throw :nope, 42; rescue => e; e.class; end)
v = begin; throw :y; rescue UncaughtThrowError => e; e.tag; end
p v
w = begin; throw :z, 7; rescue UncaughtThrowError => e; e.value; end
p w
