# GAP -- imported from the spinel corpus at c55d9bdb.
# %.2f rounds half-to-even on the decimal string; ruby rounds the binary
# double, giving 2.68 for 2.675.
#
# Ruby rounds a fixed-precision float conversion on the shortest round-trip
# decimal representation (ties to even), not on the exact binary value: 2.675 is
# stored as 2.67499999999999982, so C's printf answers 2.67 where Ruby answers
# 2.68, and 2.345 the other way round (#3958).
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
