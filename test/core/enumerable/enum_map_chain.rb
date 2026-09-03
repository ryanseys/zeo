a = [1, 2, 3]
p a.each_index.map { |c| c }
p a.each_index.collect { |c| c * 10 }
p a.each_with_index.map { |v, i| v + i }
p a.each_with_index.flat_map { |v, i| [v, i] }
p a.each_index.filter_map { |c| c if c > 0 }
__END__
[0, 1, 2]
[0, 10, 20]
[1, 3, 5]
[1, 0, 2, 1, 3, 2]
[1, 2]
