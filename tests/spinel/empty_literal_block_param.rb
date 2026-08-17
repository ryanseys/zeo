# An unused block parameter over an empty literal has no element type to infer
# from, so it is pruned and has no C declaration: the binding must be skipped
# rather than name an undeclared variable (#3853).
p([].each_with_index { |x, i| p i })
p([1, 2].each_with_index { |x, i| p [x, i] })
p([].each_with_index { |x, i| p x })
p([].each { |x| p x })
p([].map { |x| x })
h = {}
p(h.each_with_index { |pair, i| p i })
