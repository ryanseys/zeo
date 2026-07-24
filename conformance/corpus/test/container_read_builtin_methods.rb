# Methods on values read back out of containers (poly-typed receivers whose
# runtime kind is a builtin): map! writes back to the original typed array,
# Range sums/covers, Time subtracts, sum takes a seed, Integer#gcdlcm works
# on nested block params (#3234).
m = [[1, 2], [3, 4]]; m[1].map! { |x| x * 10 }; p m
r = [(1..5)][0]; p r.sum
c = [(10..20)][0]; p c.cover?(15)
p c.cover?(25)
arr = [Time.at(0), Time.at(3600)]; a = arr[0]; b = arr[1]
p(b - a)
rows = [[Rational(1, 2), Rational(1, 3)]]; row = rows[0]
p row.sum(Rational(0))
pairs = [[48, 36]]; pairs.each { |x, y| p x.gcdlcm(y) }
fs = [[1.5, 2.5]]; fs[0].map! { |x| x * 2 }; p fs
