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
