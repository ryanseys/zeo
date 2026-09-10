# each_index.map and each_with_index.sum inside a map over rows, and each_index on a plain array.
p [[1, 2]].map { |row| row.each_index.map { |c| c } }
# wrong value (no error):
p [[10, 20, 30]].map { |row| row.each_with_index.sum { |v, i| v * i } }
# Ruby: [80]

# front-end reject of a statically-typed receiver:
a = [1, 2]
p a.each_index.map { |c| c }
# Ruby: [0, 1]
__END__
[[0, 1]]
[80]
[0, 1]
