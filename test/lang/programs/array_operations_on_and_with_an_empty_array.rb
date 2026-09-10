# transpose, fill, rotate, product and zip, each with an empty operand or
# receiver.
p([].transpose)
p([].fill(1))
p([].rotate(3))
a = [1, 2, 3]
p(a.product([]))
p([].product([1, 2]))
p([1, 2].zip([]))
__END__
[]
[]
[]
[]
[]
[[1, nil], [2, nil]]
