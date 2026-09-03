# `::Integer` (and other `::Name` top-level anchors) resolve to the builtin
# class in every position: is_a?/kind_of?/instance_of? arguments, `===`
# receivers, and as a first-class Class value.

p 5.is_a?(::Integer)
p "x".is_a?(::String)
p 3.14.kind_of?(::Numeric)
p 5.instance_of?(::Integer)
p(::Integer === 7)
p(::String === "a")
p ::Integer
p ::Array.new(2, 0)
__END__
true
true
true
true
true
true
Integer
[0, 0]
