p [[1, 2]].map { |row| row.each_index.map { |c| c } }
# wrong value (no error):
p [[10, 20, 30]].map { |row| row.each_with_index.sum { |v, i| v * i } }
# Ruby: [80]   Spinel: [nil]

# front-end reject of a statically-typed receiver:
a = [1, 2]
p a.each_index.map { |c| c }
# Ruby: [0, 1]   Spinel: spinel: unsupported p argument: node N (CallNode `map`) recv=CallNode
