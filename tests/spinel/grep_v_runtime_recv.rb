# receiver routed through a method param exercises the runtime loop path
def gv(a) = a.grep_v(2)
def gp(a) = a.grep(2)
def gvr(a) = a.grep_v(2..3)

p gv([1, 2, 3, 2, 4])
p gp([1, 2, 3, 2, 4])
p gvr([1, 2, 3, 4, 5])

# literal receiver, integer pattern
p [1, 2, 3, 2, 4].grep_v(2)

# poly (mixed) array with an integer pattern
def gpoly(a) = a.grep_v(2)
p gpoly([1, "x", 2, 3, 2])
