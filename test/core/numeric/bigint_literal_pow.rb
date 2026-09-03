# A literal integer power whose result exceeds int64 is a Bignum in every
# overflow mode (CRuby). A power that fits stays a plain Integer.
puts 10 ** 30
puts 2 ** 70
puts 2 ** 64
puts 3 ** 40
puts 2 ** 10
puts 7 ** 2
y = 10 ** 25
puts y + 1
puts y * 2
__END__
1000000000000000000000000000000
1180591620717411303424
18446744073709551616
12157665459056928801
1024
49
10000000000000000000000001
20000000000000000000000000
