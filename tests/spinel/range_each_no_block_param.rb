p(catch(:n) { (1..2).each { throw :n, 7 } })
v = catch(:n2) { x = (1..2).each { throw :n2, 7 }; x }; p v
p(catch(:m) { [1, 2].each { |i| throw :m, i } })
v2 = catch(:t) { [1, 2].each { |i| throw :t, i } }; p v2
