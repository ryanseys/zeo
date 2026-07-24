f = -> (x) { x * 2 }
g = -> (x) { x + 1 }
puts (f << g).call(3)
puts (f >> g).call(3)

p1 = proc { |x| x * 2 }
p2 = proc { |x| x + 1 }
puts (p1 << p2).call(3)
puts (p1 >> p2).call(3)
