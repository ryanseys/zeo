# Float parsing and shortest-round-trip printing, both linked into the binary.
puts 0.1 + 0.2
puts 1.0 / 3
puts 1e300 * 10
puts (2.0**-1074)
puts format("%.10f", Math::PI)
puts "3.14159".to_f
__END__
0.30000000000000004
0.3333333333333333
1.0e+301
5.0e-324
3.1415926536
3.14159
