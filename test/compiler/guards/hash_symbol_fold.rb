p({ a: 1 }.inject(:+))
p({ a: 1, b: 2 }.reduce(:+))
a477 = { a: 1 }
c477 = (a477.inject(:+))
p c477
p({ a: 1, b: 2 }.inject(0) { |acc, (_k, v)| acc + v })
p([1, 2].inject(:+))
__END__
[:a, 1]
[:a, 1, :b, 2]
[:a, 1]
3
3
