rings = [[[[-63.1269583, 46.2338358], [-63.1260111, 46.234436]]]]
array = rings.flat_map { |ring| ring.first.map &:first }
p array.sum
p 7.0.fdiv 3
p array.sum.fdiv array.size
vals = [1, 2.5, 3]
p vals[0].fdiv(vals[2])
p vals.sum.fdiv(2)
p 7.fdiv(2)
__END__
-126.2529694
2.3333333333333335
-63.1264847
0.3333333333333333
3.25
3.5
