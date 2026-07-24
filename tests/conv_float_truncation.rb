# Floats truncate at NUM2LONG index sites (CRuby's rule).
p "ab" * 2.9
p [1, 2] * 2.9
p [10, 20, 30][1.7]
p [10, 20, 30].first(2.9)
p [1, 2, 3].take(2.9)
p [1, 2, 3].drop(1.5)
p [1, 2, 3, 4].values_at(1.9)
p [1, 2, 3].pop(2.9)
p "abcde"[1.5, 2.9]
p "10".to_i(2.9)
p "a".center(10.5)
p [1, 2, 3].fill(0, 1.5)
s = +"abc"
s[1.7] = "X"
p s
a = [1, 2, 3, 4]
a[1.5..2] = 9
p a
