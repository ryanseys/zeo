# `C.freeze` makes only that class frozen, and a Struct class behaves the same.
# (spinel issue #3101)
class C; end
class D; end
C.freeze
p C.frozen?
p D.frozen?
S = Struct.new(:a)
S.freeze
p S.frozen?
p String.frozen?
String.freeze
p String.frozen?
module M; end
M.freeze
p M.frozen?
__END__
true
false
true
false
true
true
