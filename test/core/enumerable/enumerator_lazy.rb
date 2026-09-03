# Enumerator::Lazy over an integer range or array: the whole chain of
# map/select/reject/filter/take_while fuses into one loop that pulls only as
# many elements as the terminal (first(n) / take(n) / to_a / force) needs, so an
# endless range works.

p (1..Float::INFINITY).lazy.map { |x| x * 2 }.first(5)
p (1..Float::INFINITY).lazy.select { |x| x.even? }.first(3)
p (1..).lazy.select { |x| x % 3 == 0 }.first(4)

# chained transforms
p (1..10).lazy.map { |x| x * x }.select { |x| x > 10 }.first(2)
p (1..100).lazy.select { |x| x.even? }.map { |x| x * 10 }.first(3)
p (1..20).lazy.reject { |x| x.even? }.first(5)

# array source + to_a / take
p [1, 2, 3, 4].lazy.map { |x| x + 1 }.to_a
p [10, 20, 30, 40, 50].lazy.select { |x| x > 25 }.to_a
p (1..10).lazy.take_while { |x| x < 5 }.to_a

# first with no count still returns a single element (existing path)
p (1..Float::INFINITY).lazy.select { |x| x > 100 }.first
__END__
[2, 4, 6, 8, 10]
[2, 4, 6]
[3, 6, 9, 12]
[16, 25]
[20, 40, 60]
[1, 3, 5, 7, 9]
[2, 3, 4, 5]
[30, 40, 50]
[1, 2, 3, 4]
101
