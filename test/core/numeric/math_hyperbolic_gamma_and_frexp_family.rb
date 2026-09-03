# Backed by the system libm CRuby also calls, so values match exactly;
# gamma uses an exact factorial table + ±0/±inf/negative-integer rules.

puts Math.tanh(1.0).round(10)
puts Math.acosh(2.0).round(10)
p Math.atanh(1.0)
puts Math.gamma(6.0)
p Math.gamma(0.0)
p Math.lgamma(-1.0)
p Math.frexp(8.0)
puts Math.ldexp(0.75, 3)
begin; Math.gamma(-2.0); rescue Math::DomainError => e; puts e.message; end
__END__
0.761594156
1.3169578969
Infinity
120.0
Infinity
[Infinity, 1]
[0.5, 4]
6.0
Numerical argument is out of domain - gamma
