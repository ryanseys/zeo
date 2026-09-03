o = Object.new
p(o <=> o)
p(o <=> Object.new)
p(o == o)
a = "x"
b = "y"
p(a <=> b)
p([1,2] <=> [1,2])
class C; end
c = C.new
p(c <=> c)
p(c <=> C.new)
__END__
0
nil
true
-1
0
0
nil
