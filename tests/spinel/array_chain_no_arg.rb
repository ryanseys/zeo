# Array#chain with no argument: the eager .chain.to_a is just the receiver's
# own elements.
p([1, 2, 3].chain.to_a)
p([1, 2].chain([3, 4]).to_a)
p([1, 2].chain([3], [4, 5]).to_a)
a = []
p a.chain.to_a
p(["x", "y"].chain.to_a)
