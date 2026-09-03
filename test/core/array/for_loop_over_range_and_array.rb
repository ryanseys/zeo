# Arithmetic on the Range case works through the emitter's numeric
# lowering and its runtime-checked fallback (`clif::expr`); an
# `Array`'s elements aren't tracked per-element (`Poly`), so the
# array case just displays each value.

sum = 0
for i in 1..5
  sum += i
end
puts sum
puts i

arr = [10, 20, 30]
for el in arr
  puts el
end
__END__
15
5
10
20
30
