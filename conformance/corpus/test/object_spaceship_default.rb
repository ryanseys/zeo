class C; end
o = C.new
p(o <=> o)
p(o <=> C.new)
class D; end
p(o <=> D.new)
