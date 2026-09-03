# Complex: imaginary literals, component-class preservation (exact
# Integer/Rational components incl. the den==1 demotion inside complex
# division), formatting (`(2/25)*i`), polar surface, and coercion errors.

c = 4i
puts c.class
p c
p 3 + 4i
p Complex(1, 2) * Complex(3, 4)
p Complex(1, 2) / Complex(3, 4)
p Complex(1, 2) / 2
p Complex(1, 2) ** 2
p Complex(1.5, -2.5)
puts Complex(1.5, -2.5)
puts Complex(3, 4).abs
p Complex(3, 4).abs2
p Complex(3, 4).rect
p Complex(1, -2).conjugate
p Complex(2, 0) == 2
p Complex(1, 2).real
p Complex(1, 2).imaginary
p 5.to_c
begin
  Complex(1, 2) + "x"
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
__END__
Complex
(0+4i)
(3+4i)
(-5+10i)
((11/25)+(2/25)*i)
((1/2)+1i)
(-3+4i)
(1.5-2.5i)
1.5-2.5i
5.0
25
[3, 4]
(1+2i)
true
1
2
(5+0i)
TypeError: String can't be coerced into Complex
