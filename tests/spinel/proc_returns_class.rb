# A proc whose value is a Class: the boxed result was cast to the sp_Class
# struct through its void pointer, which is not valid C, so the program did not
# compile. The runtime already had the reader.
f = -> { 1.class }
p f.call
g = -> (x) { x.class }
p g.call("s")
p g.call([1])
h = proc { String }
p h.call
