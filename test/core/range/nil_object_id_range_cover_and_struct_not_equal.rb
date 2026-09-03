# nil.object_id is 4 (CRuby 4.0.6). Range#cover? accepts a Range argument
# (containment). Struct#!= negates the struct's value == (not identity).

p nil.object_id
p((1..5).cover?(2..4))
p((1..5).cover?(0..4))
p((1..5).cover?(2..6))
S = Struct.new(:a, :b)
x = S.new(5, 6)
p(x == S.new(5, 6))
p(x != S.new(5, 6))
p(x != S.new(5, 9))
__END__
4
true
false
false
true
false
true
