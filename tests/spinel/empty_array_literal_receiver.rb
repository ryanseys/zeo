# An empty `[]` literal used directly as a receiver has no element type, and
# the forms below had no arm that would claim it, so the call was refused
# outright. They join the receivers that settle on an empty poly array.
p([].each_with_index.to_a)
p([].frozen?)
p([].partition { |pa| pa })
p([].take_while { |tw| tw })
p([].drop_while { |dw| dw })

# the forms that already worked keep working
p([].min)
p([].sum)
p([].size)
p([] + [1, 2])
p([].map { |ma| ma })
p([].each_slice(2).to_a)
p([].join("-"))

# and a non-empty literal receiver is unchanged
p([1, 2].each_with_index.to_a)
p([1, 2].frozen?)
p([1, 2].partition { |pb| pb > 1 })
