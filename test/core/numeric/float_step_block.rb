# Issue #870: Float#step yields floats. Pre-fix the LV slot was
# typed as mrb_int so `lv_x += 0.5` truncated to `+= 0` and looped
# forever.
result = []
1.0.step(3.0, 0.5) { |x| result << x }
puts result.inspect

# int step still works
out = []
1.step(7, 2) { |i| out << i }
puts out.inspect

# float step argument widens int recv to float
result2 = []
1.step(3.0, 0.5) { |x| result2 << x }
puts result2.inspect
__END__
[1.0, 1.5, 2.0, 2.5, 3.0]
[1, 3, 5, 7]
[1.0, 1.5, 2.0, 2.5, 3.0]
