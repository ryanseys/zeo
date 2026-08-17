# take_while / drop_while whose block answers a boxed value: `!(rbval)` is not
# valid C -- the value is a struct -- so the loop condition did not compile.
p([1,nil,2].take_while { |q| q })
p([1,nil,2].drop_while { |q| q })
p([nil,1].take_while { |w| w })
p([1,2,3].take_while { |e| e < 3 })
