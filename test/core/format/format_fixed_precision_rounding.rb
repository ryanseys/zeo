puts format("%.2f", 2.675)
puts format("%.1f", 0.15)
vals = [2.675, 1.005, 8.835, 0.145, 1.115, 2.345, 3.045, 1.255, 0.615, 4.985]
puts vals.map { |v| format("%.2f", v) }.join(" ")
puts [0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.75, 0.85].map { |v| format("%.1f", v) }.join(" ")
puts [-2.675, -2.345, -0.15].map { |v| format("%.2f", v) }.join(" ")

# a precision past the shortest digits keeps the exact expansion
puts format("%.20f", 0.1)
puts format("%.17f", 2.675)

# the field machinery is unchanged
puts format("%08.2f", -2.675)
puts format("%+.2f", 2.675)
puts format("%10.2f|", 2.675)
puts format("%-10.2f|", 2.675)
puts format("%.0f %.0f %.0f", 2.5, 3.5, 0.5)
puts format("%f", 1.0 / 3)
puts format("%.2f", 0.005)
puts format("%.2e %.3g", 2.675, 2.675)

printf("%.2f %.2f\n", 2.675, 2.345)
puts "%.2f" % 2.675
__END__
2.68
0.2
2.68 1.00 8.84 0.14 1.12 2.34 3.04 1.26 0.62 4.98
0.2 0.2 0.4 0.4 0.6 0.6 0.8 0.8
-2.68 -2.34 -0.15
0.10000000000000000555
2.67499999999999982
-0002.68
+2.68
      2.68|
2.68      |
2 4 0
0.333333
0.01
2.68e+00 2.68
2.68 2.34
2.68
