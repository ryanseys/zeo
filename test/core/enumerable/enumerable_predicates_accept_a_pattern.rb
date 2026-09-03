p [1, 2, 3].any?(Integer)
p [1, "a", 3].all?(Integer)
p [1, 2, 3].none?(String)
p [1, 2, 3].one?(2)
p [1, 2, 3].any?(4..10)
p %w[foo bar].all?(/o|a/)
p({ a: 1 }.any?([:a, 1]))
__END__
true
false
true
true
false
true
true
