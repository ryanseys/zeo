# `first` needs no stage between it and the lazy source: `e.lazy.first(2)` is
# as well defined as `e.lazy.map { }.first(2)`, but only the staged form was
# recognized and the bare one answered nil.
p([1, 2, 3].each.lazy.first(2))
p([1, 2, 3].each.lazy.first)
p([1, 2, 3].lazy.first(2))
p([1, 2, 3].each.lazy.to_a)
p([1, 2, 3].each.to_a)
p((1..9).each.lazy.first(3))
p([1, 2, 3].each.lazy.map { |x| x * 2 }.first(2))
p((1..Float::INFINITY).lazy.first(3))
