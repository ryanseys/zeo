# Array#product on two int_arrays: Cartesian product.
puts [1, 2].product([3, 4]).inspect
puts [1, 2, 3].product([10, 20]).inspect
# Empty other → empty product.
puts [1, 2].product([]).inspect
__END__
[[1, 3], [1, 4], [2, 3], [2, 4]]
[[1, 10], [1, 20], [2, 10], [2, 20], [3, 10], [3, 20]]
[]
