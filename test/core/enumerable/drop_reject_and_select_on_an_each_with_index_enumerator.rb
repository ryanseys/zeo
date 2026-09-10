# drop, reject and select chained onto each_with_index materialize the pairs,
# and map over the result binds both halves.
a = [1.0, 2.0, 3.0]
p a.each_with_index.drop(1).map { |c, i| c * i }
p a.each_with_index.drop(1)
p a.each_with_index.reject { |c, i| i == 0 }
p a.each_with_index.select { |c, i| i > 0 }
__END__
[2.0, 6.0]
[[2.0, 1], [3.0, 2]]
[[2.0, 1], [3.0, 2]]
[[2.0, 1], [3.0, 2]]
