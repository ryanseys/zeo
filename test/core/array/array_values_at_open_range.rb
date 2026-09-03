a = [1, 2, 3, 4, 5]
p a.values_at(2..)
p a.values_at(2...)
p a.values_at(..2)
p a.values_at(...2)
p a.values_at(nil..2)
p a.values_at(1..3)
p a.values_at(1..9)
p a.values_at(-2..)
p a.values_at(7..9)
p a.values_at(0, 2..3)
p([1, 2, 3, 4, 5].fill(9, 2..))
s = ["a", "b", "c"]
p s.values_at(1..)
p s.values_at(..1)
__END__
[3, 4, 5]
[3, 4, 5]
[1, 2, 3]
[1, 2]
[1, 2, 3]
[2, 3, 4]
[2, 3, 4, 5, nil, nil, nil, nil, nil]
[4, 5]
[nil, nil, nil]
[1, 3, 4]
[1, 2, 9, 9, 9]
["b", "c"]
["a", "b"]
