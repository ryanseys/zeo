# Numeric#step accepts positional (limit, step) and/or keyword (by:, to:) forms.
1.step(by: 2, to: 9) { |i| print i, " " }
puts

1.step(9, 2) { |i| print i, " " }
puts

1.step(to: 5) { |i| print i, " " }
puts

10.step(by: -3, to: 1) { |i| print i, " " }
puts

# Float step moves the whole iteration into the Float domain.
1.step(2.0, 0.5) { |i| print i, " " }
puts

# Positional limit with a keyword step.
0.step(10, by: 5) { |i| print i, " " }
puts
