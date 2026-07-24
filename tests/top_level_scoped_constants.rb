# A `::Name` top-level anchor resolves to the builtin (or top-level) class in
# every position -- CRuby represents top-level constants as living on Object,
# so `::Integer` is `Object::Integer` is the `Integer` class.

# As an is_a?/kind_of?/instance_of? argument.
p 5.is_a?(::Integer)
p "x".is_a?(::String)
p [1].is_a?(::Array)
p 3.14.kind_of?(::Numeric)
p nil.is_a?(::NilClass)
p 5.is_a?(::Comparable)
p 5.instance_of?(::Integer)

# As a `===` receiver (case/when membership).
p(::Integer === 7)
p(::Integer === "x")
p(::Float === 1.5)
p(::Symbol === :s)

# As a first-class Class value.
p ::Integer
p ::String.name
p ::Array.new(3, :x)

# A user top-level class works the same.
class Widget; end
p Widget.new.is_a?(::Widget)
