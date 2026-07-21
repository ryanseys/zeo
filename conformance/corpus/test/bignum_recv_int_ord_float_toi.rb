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
