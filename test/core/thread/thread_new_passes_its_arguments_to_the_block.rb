# One, two and three arguments, and an array as a single one.
p Thread.new(7, 8) { |a, b| [a, b] }.value
p Thread.new(7) { |a| a }.value
r = (Thread.new(1, 2, 3) { |a, b, c| a + b + c }.value rescue $!.class); p r
p Thread.new([1, 2]) { |a| a }.value
__END__
[7, 8]
7
6
[1, 2]
