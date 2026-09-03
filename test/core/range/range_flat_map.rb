# Range#flat_map maps each element through the block then flattens one level.
def s(x); x; end
p s(1..5).flat_map { |x| [x] }
p s(1..3).flat_map { |x| [x, x * 10] }
p s(1..4).flat_map { |x| x.even? ? [x] : [] }
__END__
[1, 2, 3, 4, 5]
[1, 10, 2, 20, 3, 30]
[2, 4]
