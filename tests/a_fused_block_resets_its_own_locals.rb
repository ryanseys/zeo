# A name first assigned INSIDE a fused block is fresh on every iteration,
# even though the splice shares the enclosing scope where it was hoisted.
out = []
3.times do |i|
  x = i if i.odd?
  out << x
end
p out
res = []
(1..4).each do |i|
  seen = true if i > 2
  res << [i, seen]
end
p res
total = 0
sum = []
4.times do |i|
  total += i
  tmp = i * 2 if i != 1
  sum << tmp
end
p [total, sum]
