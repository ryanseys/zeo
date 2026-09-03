# Math module functions + Math::DomainError + the Float constants, and
# the Kernel conversion functions with CRuby's exact failure shapes.

puts Math::PI
puts Math::E
puts Math.sqrt(16)
puts Math.sqrt(2)
puts Math.cbrt(27)
puts Math.log2(8)
puts Math.log(Math::E)
puts Math.log(8, 2)
puts Math.hypot(3, 4)
puts Math.atan2(1, 1)
begin
  Math.sqrt(-1)
rescue Math::DomainError => e
  puts "Math::DomainError: #{e.message}"
end
puts Float::INFINITY
puts Float::EPSILON
puts Float::MAX
puts Float::DIG
puts Float::RADIX
puts Integer("42")
puts Integer("ff", 16)
puts Integer("0x1A")
puts Integer(" -4_2 ")
puts Integer(3.9)
puts Float("1.5e3")
puts Float(2)
puts String(42)
puts Array(nil).inspect
puts Array(1..3).inspect
puts Array(5).inspect
puts Hash(nil).inspect
p Rational(3, 4)
p Complex(1, 2)
begin
  Integer("nope")
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
begin
  Integer(nil)
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
begin
  Float("x")
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
__END__
3.141592653589793
2.718281828459045
4.0
1.4142135623730951
3.0
3.0
1.0
3.0
5.0
0.7853981633974483
Math::DomainError: Numerical argument is out of domain - sqrt
Infinity
2.220446049250313e-16
1.7976931348623157e+308
15
2
42
255
26
-42
3
1500.0
2.0
42
[]
[1, 2, 3]
[5]
{}
(3/4)
(1+2i)
ArgumentError: invalid value for Integer(): "nope"
TypeError: can't convert nil into Integer
ArgumentError: invalid value for Float(): "x"
