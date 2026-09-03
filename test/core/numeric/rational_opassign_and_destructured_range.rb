# `rational += <boxed>` and a Range bounded by a destructured block parameter.
b = [Rational(1), Rational(2)]
acc = Rational(0)
acc += b[0]
p acc
acc += b[1]
p acc
acc -= b[0]
p acc
acc *= b[1]
p acc
acc /= b[1]
p acc

cx = [Complex(1, 1)]
z = Complex(0, 0)
z += cx[0]
p z
z *= cx[0]
p z

[[2026, 7]].each do |(y, m)|
  p (1...m).reduce(0) { |s, mm| s + mm }
  p (1...m).inject { |s, mm| s + mm }
  p (1..m).reduce(0) { |s, mm| s + mm }
  p (1...m).sum
  p (1...m).to_a
  p (1...m).select { |v| v.even? }
end
__END__
(1/1)
(3/1)
(2/1)
(4/1)
(2/1)
(1+1i)
(0+2i)
21
21
28
21
[1, 2, 3, 4, 5, 6]
[2, 4, 6]
