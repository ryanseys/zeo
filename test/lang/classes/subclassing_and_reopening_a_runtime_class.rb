# `class Point < Struct.new(:x, :y)` -- a superclass that only exists at RUN
# time (a `Struct`/`Data` class, a `Class.new`, a constant holding either).
# The subclass can't be one of the statically emitted Rust structs, so it is
# minted at runtime too; `super`, `superclass`, and a namespaced name all
# have to keep working through it. `class D ... end` reopening such a
# constant installs onto the existing class rather than registering a fresh
# (memberless) one, so a generated reader still resolves in the body.

class Point < Struct.new(:x, :y)
  def dist2; x * x + y * y; end
end
p Point.new(3, 4).dist2
p Point.superclass.ancestors.include?(Struct)

Base = Class.new do
  def greet; "base"; end
end
class Child < Base
  def greet; "child+" + super; end
end
p Child.new.greet
p Child.superclass.equal?(Base)

D = Data.define(:v)
class D
  def double; v * 2; end
end
p D.new(5).double

module NS; end
class NS::Item < Struct.new(:a)
  def show; "a=#{a}"; end
end
p NS.constants
p NS::Item.name
p NS::Item.new(1).show
__END__
25
true
"child+base"
true
10
[:Item]
"NS::Item"
"a=1"
