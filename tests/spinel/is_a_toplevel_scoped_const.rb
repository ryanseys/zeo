p 5.is_a?(::Integer)
p "x".is_a?(::String)
p [1].is_a?(::Array)
class Foo; end
p Foo.new.is_a?(::Foo)
p 3.14.kind_of?(::Numeric)
p nil.is_a?(::NilClass)
p 5.is_a?(::Comparable)
