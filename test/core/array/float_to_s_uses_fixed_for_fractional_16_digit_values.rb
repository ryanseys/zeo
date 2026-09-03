# Ruby #2593: a 16-digit-integer-part float WITH a fraction prints fixed,
# while an integer-valued 16-digit double stays scientific.

puts 4503599627370495.5
puts 1234567890123456.7
puts(-4503599627370495.5)
puts 9007199254740992.0
puts 1e15
puts 1e16
__END__
4503599627370495.5
1234567890123456.8
-4503599627370495.5
9.007199254740992e+15
1.0e+15
1.0e+16
