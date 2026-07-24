# Issues #840 #841 #872: Imaginary, Rational literals + Integer#quo.
# Both surface as value-type structs (sp_Complex, sp_Rational).
# Integer#quo returns a Rational. v1 scope: literal forms +
# .inspect/.to_s; arithmetic between mixed Integer/Rational/Complex
# is deferred to a follow-up.

# Imaginary literals
puts 1i.inspect
puts 2i.inspect
puts 3.0i.inspect

# Imaginary parts via .real / .imag
puts 5i.real
puts 5i.imaginary

# Rational literals (reduced by prism)
puts 3r.inspect
puts 4r.inspect

# Integer#quo
puts 7.quo(2).inspect
puts 10.quo(4).inspect
puts 100.quo(5).inspect

# numerator / denominator
puts 7.quo(2).numerator
puts 7.quo(2).denominator
