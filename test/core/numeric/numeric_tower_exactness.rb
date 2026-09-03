p 42.size
p (2**64).size
p (2**64 - 1).size
p (2**128).size
p 0.3.rationalize
p 2.5.rationalize
p 3.14159.rationalize
p 1.333.rationalize(0.01)
p Rational(2.5)
p Rational(1.5, 0.5)
begin
  Complex(nil)
rescue TypeError => e
  puts e.message
end
p 2 ** Complex(0, 1)
p Complex(6, 0).to_r
p Complex(3, 4).numerator
p 7.div(Rational(2))
p 10.div(Rational(3, 2))
__END__
8
9
8
17
(3/10)
(5/2)
(314159/100000)
(4/3)
(5/2)
(3/1)
can't convert nil into Complex
(0.7692389013639721+0.6389612763136348i)
(6/1)
(3+4i)
3
6
