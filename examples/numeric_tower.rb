# Phase 17.1 -- the numeric tower: full-bignum Integer, Rational, Complex,
# the CRuby-exact coercion matrix, Math, and Kernel conversions.

r = 1
i = 2
while i <= 30
  r = r * i
  i += 1
end
puts r
puts 2 ** 100
puts 9_223_372_036_854_775_807 + 1
puts (2 ** 70) > (2 ** 69)

p 2 ** -2
p 1.quo(3)
p Rational(4, 8) + Rational(1, 3)
p Rational(1, 2) + 0.5
p 3.75r * 4

c = Complex(1, 2) * Complex(3, 4)
p c
p c.real
p c.imaginary
p 4i + 3
p Complex(1, 2) / 2

puts 7.divmod(3).inspect
puts (-7).divmod(3).inspect
puts 10.digits.inspect
puts 4.gcd(6)
puts 255.to_s(16)
puts 25.round(-1)
puts 3.14159.round(2)
puts 7.fdiv(2)
puts 1e20.to_i
puts 0.125.to_r.inspect

puts Math.sqrt(16)
puts Math.hypot(3, 4)
puts Math::PI
begin
  Math.sqrt(-1)
rescue Math::DomainError => e
  puts "domain: #{e.message}"
end

puts Integer("ff", 16)
puts Integer(" -4_2 ")
puts Float("1.5e3")
begin
  Integer("nope")
rescue ArgumentError => e
  puts "bad: #{e.message}"
end
