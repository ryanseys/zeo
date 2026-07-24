# any?/all?/none?/one? with a block that takes NO parameter. The synthetic
# iteration slot must not be bound (an empty block can't reference the yielded
# value), otherwise codegen emits an undeclared `lv__x` write. Covers empty and
# non-empty arrays plus the range path.
p([].any? { true })
p([1, 2].any? { true })
p([].all? { false })
p([1, 2].all? { true })
p([1, 2].none? { false })
p([1, 2, 3].one? { true })
p([1].one? { true })
p((1..3).any? { true })
p((1..3).all? { false })
p((1..3).none? { false })
