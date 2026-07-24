p(proc { next 5 }.call)
p(proc { |x| next x * 2 }.call(4))
p([1,2,3].map { |x| next x + 1 })
pr = proc { |x| next 0 if x < 0; x }
p pr.call(-3)
p pr.call(3)
