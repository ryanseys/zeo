# The class-method channel. A box's `def self.x` on a SHARED class is the
# box's alone: main sees neither the method nor a `respond_to?` for it,
# while the box still reaches `Array`'s ordinary rows.
#
# This is the shape that leaked -- `class_methods` was keyed by name only,
# where the instance channel had carried `(box, name)` from the start.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class Array; def self.zzz = 9; end")
p Array.respond_to?(:zzz)
p(begin; Array.zzz; rescue NoMethodError; :nome; end)
p b.eval("Array.zzz")
p b.eval("Array.respond_to?(:zzz)")
p b.eval("Array.new(2, 7)")
__END__
false
:nome
9
true
[7, 7]
