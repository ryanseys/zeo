# Shapes whose generated C carried a pointer of the wrong type: each read one
# struct through another's layout, and each was only a compiler warning before
# (#3975's second observation -- the build now fails on one).
x = case "foo"
    when ["foo", "foo"] then "bad"
    when "foo" then "good"
    end
p x

# `[].freeze.rotate`: the empty literal was built as the int-array default and
# handed to the poly-array path
p(-> { [].freeze.rotate }.call)
p [2].freeze.rotate(2)
p [1, 2, 3].freeze.rotate(-3)

# product with a block answers self, and its empty-array argument is poly
arr = [1, 2]
seen = []
p arr.product([3, 4], [5]) { |t| seen << t }.equal?(arr)
p seen
p arr.product([]) { }.equal?(arr)
p arr.product([3, 4]) { }.equal?(arr)

# a massign rest slices the SOURCE array's kind
*a, b, c = *1
p [a, b, c]
*d, e = *[1, 2, 3]
p [d, e]
*f, g = "x", "y", "z"
p [f, g]
