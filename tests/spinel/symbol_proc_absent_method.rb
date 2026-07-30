# max_by / min_by / sort_by with a symbol-proc naming a method the element does
# not have compiles and raises NoMethodError, as map with the same proc does.
a = ([1, 2].max_by(&:foo) rescue $!.class); p a
b = ([1, 2].min_by(&:foo) rescue $!.class); p b
c = ([1, 2].sort_by(&:foo) rescue $!.class); p c
d = ([1, 2].map(&:foo) rescue $!.class); p d
e = (["a", "b"].max_by(&:nope) rescue $!.class); p e
p [1, 2].max_by(&:abs)
p [3, 1, 2].sort_by { |x| -x }
p [1, 2, 3].min_by(&:abs)
