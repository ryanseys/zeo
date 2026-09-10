# `take_while.with_index` stops on the index rather than the value.
p [3, 3, 5, 2].take_while.with_index { |v, i| i < 2 }
__END__
[3, 3]
