# Local types are recomputed from scratch each inference round, in node order.
# A local assigned inside an iterator block sits at a higher node id than the
# write that receives the iterator's value, so that write derived its type with
# the block's locals still reset -- narrower than what the block really yields.
# The result array was then declared as an Integer array while the loop pushed
# boxed values into a poly one, printing 16 zero-padded elements. #3463.
ws = [[1, 2], [3, 4]]
p([0, 1].map { |i| w = ws.select { |a, _| a > 0 }.last; w ? w[0] : 9 })
p([0, 1].map { |i| w = ws.select { |a, _| a > 0 }.first; w ? w[0] : 9 })
p([0, 1].map { |i| w = ws.reject { |a, _| a < 0 }.last; w ? w[0] : 9 })
p([0, 1].map { |i| w = ws.select { |a, _| a > 0 }.last; if w then w[0] else 9 end })
p((0..1).map { |i| w = ws.select { |a, _| a > 0 }.last; w ? w[0] : 9 })
p([0, 1].each_with_object([]) { |i, acc| w = ws.select { |a, _| a > 0 }.last; acc << (w ? w[0] : 9) })
p([0, 1].map { |i| ws.select { |a, _| a > 0 }.last[0] })
ws2 = [1, 2]
p([0, 1].map { |i| w = ws2.select { |a| a > 0 }.last; w ? w + 1 : 9 })
def f(ws) = (w = ws.select { |a, _| a > 0 }.last; w ? w[0] : 9)
p([0, 1].map { |i| f([[1, 2], [3, 4]]) })
