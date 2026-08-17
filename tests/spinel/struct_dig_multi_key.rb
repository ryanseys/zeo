Node001 = Struct.new(:val, :kids)
t001 = Node001.new(1, [Node001.new(2, [])])
p t001.dig(1, 0, 0)
p t001.dig(1, 0)
p t001.dig(1, 0).val
p t001.kids[0].dig(0)
p t001.dig(:kids, 0, :val)
p t001.dig(0)
