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
