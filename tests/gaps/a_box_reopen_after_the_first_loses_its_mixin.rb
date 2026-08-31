b = Ruby::Box.new
b.eval("module M1; def one = 1; end
module M2; def two = 2; end
class Array; include M1; end
class Array; include M2; end")
p b.eval("[1, 2].one")
p b.eval("[1, 2].two")
