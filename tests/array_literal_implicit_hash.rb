# A brace-less hash as the last element of an ARRAY literal. Ruby collapses it
# into one ordinary Hash element, the same value the braced form builds --
# `[1, k: 2]` is `[1, {k: 2}]`, two elements, not three.
#
# prism spells it `KeywordHashNode` rather than `HashNode`, and zeo lowered
# that shape only in a CALL's argument list (where it means keyword arguments,
# which bind differently). Anywhere else it fell through to the generic
# rejection -- so activerecord's
#
#   [:change_column_default, [table, column, from: options[:to], to: options[:from]]]
#
# did not compile, and neither did `return k: 1`.

p [1, 2, a: 3, b: 4]
p [a: 1]
p [1, 2, a: 3, b: 4].length

# The values are ordinary expressions, including index reads -- the shape
# activerecord's command recorder inverts a migration with.
options = { to: "new", from: "old" }
p [:change_column_default, ["posts", "title", from: options[:to], to: options[:from]]]

# A `**splat` is the same node with only an AssocSplatNode inside it, which is
# why graphql's `[*batch_args, **batch_kwargs]` failed with the same message.
extra = { c: 3 }
rest = [1, 2]
p [*rest, **extra]
p [1, **extra, d: 4]

# Insertion order and last-wins merging survive, since this reuses the same
# builder the braced literal and call kwargs use. The overlapping key comes
# from a variable rather than a second literal, so this stays a test of the
# lowering and not of ruby's duplicate-key warning.
later = { x: 2, y: 3 }
p [x: 1, **later]

# Nested one level down, which is where activerecord's actually sits.
p [[1, k: 2], [3, k: 4]]
