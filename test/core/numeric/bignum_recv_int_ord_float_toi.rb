# Bignum-receiver conveniences (digits/to_s(base)/even?/odd?/abs),
# Integer#ord/#integer?, and Float#to_i beyond int64 raising loudly
# (Bignum promotion for statically-int results is tracked on #2024).
x = 2 ** 100
p(x.digits)
p(x.to_s(16))
p(x.even?)
p(x.odd?)
p(x.abs)
y = x * -1
p(y.abs)
p(x.to_s(2).length)
p(5.integer?)
p(65.ord)
n = 7
p n.ord
p n.integer?
begin
  (1.5e20).to_i
rescue RangeError => e
  puts "RangeError"
end
p 3.9.to_i
p(-2.9.to_i)
__END__
[6, 7, 3, 5, 0, 2, 3, 0, 7, 6, 9, 4, 1, 0, 4, 9, 2, 2, 8, 2, 2, 0, 0, 6, 0, 5, 6, 7, 6, 2, 1]
"10000000000000000000000000"
true
false
1267650600228229401496703205376
1267650600228229401496703205376
101
true
65
7
true
3
-2
