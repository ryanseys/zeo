# GAP -- imported from the spinel corpus at c55d9bdb.
# A Set algebra operation over a non-Set Enumerable does not preserve
# ruby's result order.
#
require 'set'

# The intersection and difference ENUMERATE the operand and consult the
# receiver, so a Hash operand contributes its [key, value] pairs and the result
# follows the operand's order. Asking the operand for #include? instead read a
# Hash by key, and kept the receiver's order.
p((Set[1, 2] & { 1 => :a }).to_a)
p((Set[1, 2] - { 1 => :a }).to_a)
p((Set[[:a, 1]] & { a: 1 }).to_a)
p((Set[[:a, 1]] - { a: 1 }).to_a)

p((Set[1, 2] & [2, 1]).to_a)
p((Set[1, 2, 3] & [3, 1]).to_a)
p((Set[1, 2] - [2]).to_a)
p((Set[1, 2] & Set[2, 3]).to_a)
p((Set[1, 2] - Set[2, 3]).to_a)
p((Set[1, 2] & []).to_a)
p((Set[1, 2] - []).to_a)
p((Set[1, 2] | { 3 => :c }).to_a)
p((Set[1, 2] ^ Set[2, 3]).to_a)
