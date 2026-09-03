p (1..Float::INFINITY).lazy.select(&:even?).map { |x| x * x }.first(3)
p [1, 2, 3, 4].lazy.map { |x| x + 1 }.to_a
p [1, 2, 3].lazy.class.name
__END__
[4, 16, 36]
[2, 3, 4, 5]
"Enumerator::Lazy"
