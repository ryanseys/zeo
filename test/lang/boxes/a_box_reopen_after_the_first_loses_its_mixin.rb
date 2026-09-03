#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("module M1; def one = 1; end
module M2; def two = 2; end
class Array; include M1; end
class Array; include M2; end")
p b.eval("[1, 2].one")
p b.eval("[1, 2].two")
p b.eval("Array.ancestors.take(3).inspect")
p b.eval("[Array.include?(M1), Array.include?(M2)].inspect")
p b.eval("[1, 2].is_a?(M1)")
p b.eval("Array.method_defined?(:one)")
p b.eval("[1, 2].respond_to?(:two)")
p b.eval("Array.instance_method(:one).owner.to_s")
p b.eval("[1, 2].method(:two).owner.to_s")

c = Ruby::Box.new
c.eval("module P1; def label = 'from P1'; end
class Hash; def label = 'own'; end
class Hash; prepend P1; end")
p c.eval("({}).label")
p c.eval("Hash.ancestors.first.to_s")
__END__
1
2
"[Array, M2, M1]"
"[true, true]"
true
true
true
"M1"
"M2"
"from P1"
"P1"
