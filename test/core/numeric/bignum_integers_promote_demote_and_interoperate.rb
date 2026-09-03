# Bignum end-to-end: overflow promotion + demotion round trips, big
# literals (decimal/hex/binary/underscored), interpolation, hash keys,
# Enumerable, is_a?, and the i64::MIN / -1 overflow edge.

r = 1
i = 2
while i <= 25
  r = r * i
  i += 1
end
puts r
puts 2 ** 100
puts 100000000000000000000 + 1
puts 0xff
puts 0b1010
puts 1_000_000
big = 9_223_372_036_854_775_807
puts big + 1
puts big + 1 - 1
puts (big + 1) > big
puts (big + 1).class
puts "v=#{2 ** 70}"
h = { 2 ** 70 => :big }
puts h[2 ** 70]
puts [2 ** 70, 1, 2 ** 65].min
puts (2 ** 70).is_a?(Numeric)
puts 2 ** 70 == 2 ** 70
puts(-9223372036854775808 / -1)
__END__
15511210043330985984000000
1267650600228229401496703205376
100000000000000000001
255
10
1000000
9223372036854775808
9223372036854775807
true
Integer
v=1180591620717411303424
big
1
true
true
9223372036854775808
