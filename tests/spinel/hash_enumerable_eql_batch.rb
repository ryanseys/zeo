# Hash Enumerable wiring, eql?, replace widening (KieranP #2361,#2372,#2373,#2374)
p({ a: 1 }.eql?({ a: 1 }))
p({ a: 1 }.eql?({ a: 2 }))
class NoEql
  def initialize(x); @x = x; end
end
n = NoEql.new(1)
p n.eql?(n)
p(/ab/ == /ab/)
a = /ab/
v = a.dup
p(v == a)
d = [1, 2, 3].each
w = d.dup
p(w == d)
p w.next
p({ a: 1, b: 2 }.find_index([:b, 2]))         # #2372
p({ a: 1, b: 2 }.uniq)
p({ a: 1 }.zip([9]))
p({ a: 1, b: 2 }.tally)
r = []
{ a: 1, b: 2 }.reverse_each { |k, v2| r << k }
p r
p({ 1 => 1.5, 2 => 2.5 }.value?(2.5))          # #2373
p({ 1 => 1.5, 2 => 2.5 }.has_value?(3.5))
h = { a: 1, b: 2 }                              # #2374 (used to hang)
res = h.transform_keys!(&:to_s)
p res
p h
h2 = { a: 1 }
r2 = h2.replace({ "x" => 2 })
p r2
p h2
