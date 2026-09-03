# rand(Float) truncates the bound and draws an Integer (CRuby 4.0.6);
# Integer#digits distinguishes a negative radix from a 0/1 radix.

p rand(3.5).class
p (0...3).include?(rand(3.5))
r1 = (begin; 123.digits(0); rescue ArgumentError => e; e.message; end); p r1
r2 = (begin; 123.digits(-5); rescue ArgumentError => e; e.message; end); p r2
__END__
Integer
true
"invalid radix 0"
"negative radix"
