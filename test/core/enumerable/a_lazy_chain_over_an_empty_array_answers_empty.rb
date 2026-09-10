# map, select, to_a and first over an empty array's lazy enumerator.
p([].lazy.map { |x| x * 2 }.to_a)
p([].lazy.to_a)
p([].lazy.first(2))
p([].lazy.select { |x| x > 0 }.first(2))
p([1, 2, 3].lazy.to_a)
p([1, 2, 3].lazy.map { |x| x * 2 }.to_a)
p([1, 2, 3].lazy.select { |x| x > 1 }.first(2))
__END__
[]
[]
[]
[]
[1, 2, 3]
[2, 4, 6]
[2, 3]
