p (1..Float::INFINITY).lazy.take_while { |x| x < 5 }.to_a
p (1..).lazy.take_while { |x| x * x < 30 }.to_a
p (1..Float::INFINITY).lazy.map { |x| x * 3 }.take_while { |x| x < 20 }.to_a
__END__
[1, 2, 3, 4]
[1, 2, 3, 4, 5]
[3, 6, 9, 12, 15, 18]
