# The value of a begin/rescue is the body's when the body carries one. A body
# whose last statement is a local read is typed by a write that comes later in
# the walk, so within a round it reads as "not settled yet" -- and taking the
# handler's type alone at that point made the whole begin an Integer, coercing
# the array the body really answered into an int slot.
r = (begin; a = [1]; a; rescue; 0; end)
p r

s = (begin; b = [1, 2]; b; rescue; 0; end)
p s

t = (begin; 5; rescue; 0; end)
p t

u = (begin; c = "x"; c; rescue; 0; end)
p u

v = (begin; d = [1]; d; rescue; "z"; end)
p v

w = (begin; e = { a: 1 }; e; rescue; 0; end)
p w

# a body that really has no value still takes the handler's
x = (begin; raise "boom"; rescue; 7; end)
p x
