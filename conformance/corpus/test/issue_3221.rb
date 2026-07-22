a = (2..5)
r = rand(a)
p (2..5).include?(r)
b = (5..2)          # empty range in a variable -> nil, like CRuby
p rand(b)
c = (0...3)
p (0...3).include?(rand(c))
