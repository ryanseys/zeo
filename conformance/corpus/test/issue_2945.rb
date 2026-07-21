# Array#sum with a do...end block containing a leading local assignment: the
# per-iteration leading statements must not emit a #line directive mid
# statement-expression (a stray '#').
r = [1, 2, 3].sum do |t|
  x = t * 2
  x
end
p r
f = [1.0, 2.0].sum do |t|
  y = t + 1.0
  y
end
p f
