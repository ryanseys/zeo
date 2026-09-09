# JSON.parse's container-class options: `object_class:`/`array_class:`
# build the parse result out of the given classes, and `decimal_class:`
# parses floats into it (BigDecimal is the use case). zeo ignores all
# three.
require "json"
require "ostruct"
require "bigdecimal"
p JSON.parse('{"a":1}', object_class: OpenStruct).a
h = Class.new(Hash)
p JSON.parse('{"a":1}', object_class: h).class == h
a = Class.new(Array)
p JSON.parse("[1]", array_class: a).class == a
p JSON.parse('{"x":1.5}', decimal_class: BigDecimal)["x"].class
__END__
1
true
true
BigDecimal
