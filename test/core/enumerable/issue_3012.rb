b = [10, 20, 30].lazy.map { |x| x + 1 }
p(b.first(2))
c = [1, 2, 3].lazy
p(c.map { |x| x * 2 }.to_a)
d = (1..Float::INFINITY).lazy.select { |x| x % 3 == 0 }
p(d.first(3))
e = [1, 2, 3, 4].lazy
p(e.select { |x| x.even? }.to_a)
__END__
[11, 21]
[2, 4, 6]
[3, 6, 9]
[2, 4]
