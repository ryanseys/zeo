# `take_while` and `find` terminate over an endless Enumerator -- one built by
# `Enumerator.produce`, and one whose generator block is an unbounded `loop`.
# Neither needs `.lazy` to do it: the EAGER Enumerable methods stop the source
# too, via the same break the lazy pipeline's `Flow::Stop` uses.
#
p(Enumerator.produce(1) { |n| n + 1 }.take_while { |n| n < 5 })
p(Enumerator.new { |y| i = 0; loop { y << (i += 1) } }.take_while { |n| n < 4 })
p([1, 2, 3].each.take_while { |n| n < 3 })
p(Enumerator.produce(1) { |n| n + 1 }.find { |n| n > 3 })
p([1, 2, 3].each.take_while { |n| n > 9 })
