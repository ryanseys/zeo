# Complex#abs keeps the Integer class when a component is zero and both are
# integers (Complex(0,2).abs == 2), else Float. Dividing by a real scalar
# is componentwise: a Float 0.0 divisor yields Infinity, an Integer 0
# raises. Non-finite imaginary parts render with `*i`.

p Complex(0, 2).abs
p Complex(-3, 0).abs
p Complex(3, 4).abs
p Complex(2, 0.0).abs
p Complex(0, 2).polar[0]
p(Complex(20, 40) / 0.0)
p(Complex(21, 41) / 2.0)
p(Complex(20, 40) / 4)
r = (begin; Complex(20, 40) / 0; rescue ZeroDivisionError => e; e.message; end); p r
p((Complex(3, 0) / 0.0).to_s)
__END__
2
3
5.0
2.0
2
(Infinity+Infinity*i)
(10.5+20.5i)
(5+10i)
"divided by 0"
"Infinity+NaN*i"
