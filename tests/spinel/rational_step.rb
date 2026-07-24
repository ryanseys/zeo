# Numeric#step on a Rational receiver walks the exact sequence, yielding
# Rational/Integer values through the poly numeric tower (sp_poly_add keeps a
# Rational operand rational). Block form iterates and returns self; the
# blockless form materializes the sequence. Regression for #2566.
Rational(1,2).step(Rational(5,2), 1) { |x| p x }
p Rational(1,2).step(Rational(5,2), 1).to_a
p Rational(5,2).step(Rational(1,2), Rational(-1,1)).to_a   # descending
p Rational(1,1).step(3, 1).to_a                            # Integer limit and step
p Rational(1,2).step(Rational(5,2), 1).map { |x| x * 2 }   # chained enumerator
r = Rational(0,1)
ret = r.step(Rational(1,1), Rational(1,2)) { |x| }
p ret == r                                                 # block form returns self

# step of 0 raises ArgumentError
begin
  Rational(1,2).step(Rational(5,2), 0) { |x| }
  puts "no raise"
rescue ArgumentError
  puts "zero raised"
end

# a boxed Rational also sorts/compares now (poly cmp fix)
p [Rational(5,2), Rational(1,2), Rational(3,2)].sort
