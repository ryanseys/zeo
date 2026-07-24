# A Set read out of a container (poly receiver) iterates through the
# generated to_a hook: map/select run over its elements, inject(:op)
# folds through the numeric tower (#3234).
require 'set'
h = { a: Set.new([1, 2, 3]) }
p h[:a].map { |x| x }
p h[:a].select { |x| x > 1 }
p h[:a].inject(:+)
p h[:a].inject(:*)
p [[1, 2, 3]][0].inject(:+)
