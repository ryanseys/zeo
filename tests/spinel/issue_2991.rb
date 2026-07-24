h = { a: 1, b: 2, c: 3, d: 4 }
pr = h.partition { |_k, v| v.even? }
p pr[0].to_h
p pr[1].to_h
