# Builtin-receiver method objects read a dumped CRuby arity table; attr and
# struct accessors derive theirs from the accessor shape (reader 0, writer 1).

p "hello".method(:upcase).arity
p 5.method(:+).arity
p [1].method(:size).arity
class C; attr_accessor :x; end
p C.new.method(:x).arity
p C.new.method(:x=).arity
S = Struct.new(:a)
s = S.new(1)
p s.method(:a).arity
p s.method(:a=).arity
__END__
-1
1
0
0
1
0
1
