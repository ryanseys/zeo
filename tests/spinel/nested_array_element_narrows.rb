# an element read off a nested array narrows past the first level
rows = [[2026, 5, 1]]
m = rows[0][1]
p (1...m).reduce(0) { |s, i| s + i }
p (1..m).inject(0) { |s, i| s + i }
p (1...m).reverse_each.to_a
p (1...m).sum
p (1...m).to_a
strs = [["a", "b"]]
sv = strs[0][1]
p sv.upcase
fl = [[1.5, 2.5]]
fv = fl[0][1]
p fv + 1
