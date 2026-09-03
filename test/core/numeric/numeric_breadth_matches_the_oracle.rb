# The Numeric/Integer/Float Tier A breadth: divmod matrices, the rounding
# families, gcd/lcm/digits/chr/ord, predicates, step/times/upto/downto,
# exact Float#to_r, and bignum-capable Float#to_i.

puts 7.divmod(3).inspect
puts (-7).divmod(3).inspect
puts 7.divmod(2.5).inspect
puts (-7).abs
puts 2.5.abs
puts 4.even?
puts 3.odd?
puts 5.succ
puts 5.pred
puts 65.chr
puts "A".ord
puts 10.digits.inspect
puts 255.digits(16).inspect
puts 4.gcd(6)
puts 4.lcm(6)
puts 4.gcdlcm(6).inspect
puts 255.to_s(16)
puts 10.to_s(2)
puts 25.round(-1)
puts 1234.round(-2)
puts (-15).round(-1)
puts 1234.floor(-2)
puts 1234.ceil(-2)
puts 7.fdiv(2)
puts 3.7.round
puts 3.14159.round(2)
puts (-2.7).floor
puts 2.2.ceil
puts 5.9.truncate
puts 1e20.to_i
puts 0.125.to_r.inspect
puts (1.0 / 0).infinite?
puts 1.5.nan?
puts 2.5.finite?
puts 5.zero?
puts 0.zero?
puts 5.positive?
puts (-5).negative?
puts 5.numerator
puts 5.denominator
puts 0.5.numerator
puts 0.5.denominator
puts (-7).remainder(3)
acc = []
1.step(10, 3) { |i| acc << i }
puts acc.inspect
acc2 = []
3.times { |i| acc2 << i }
2.upto(4) { |i| acc2 << i }
3.downto(1) { |i| acc2 << i }
puts acc2.inspect
puts 255.bit_length
puts 5.to_f
puts 5.to_r.inspect
__END__
[2, 1]
[-3, 2]
[2, 2.0]
7
2.5
true
true
6
4
A
65
[0, 1]
[15, 15]
2
12
[2, 12]
ff
1010
30
1200
-20
1200
1300
3.5
4
3.14
-3
3
5
100000000000000000000
(1/8)
1
false
true
false
true
true
true
5
1
1
2
-1
[1, 4, 7, 10]
[0, 1, 2, 2, 3, 4, 3, 2, 1]
8
5.0
(5/1)
