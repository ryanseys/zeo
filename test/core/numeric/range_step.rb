# `(a..b).step(k)` with no block answers something `.to_a` turns into the
# stepped values.

puts (1..10).step(3).to_a.inspect
puts (0..20).step(5).to_a.inspect

# Inclusive vs exclusive end:
puts (1...10).step(3).to_a.inspect

# step larger than range -> single element.
puts (1..10).step(100).to_a.inspect

# step exactly hits the end.
puts (1..10).step(1).to_a.inspect
__END__
[1, 4, 7, 10]
[0, 5, 10, 15, 20]
[1, 4, 7]
[1]
[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
