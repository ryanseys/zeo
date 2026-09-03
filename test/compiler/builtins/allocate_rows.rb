# `allocate` -- a blank instance with no `initialize`. zeo raised
# "allocator undefined" for `Object`, `BasicObject`, `Range` and every
# exception class, and `Class.allocate` (ruby declares it on `Class`'s OWN
# singleton) answered off the generic row.

def show(label)
  print label, ": "
  p yield
rescue Exception => e # rubocop:disable Lint/RescueException
  p [e.class, e.message]
end

p Class.singleton_methods(false).sort

# `Class.allocate` is a class with NO superclass -- not one whose superclass is
# `BasicObject`. Nothing can come of it until `initialize` runs.
c = Class.allocate
show("class") { c.class }
show("is_a") { [c.is_a?(Class), c.is_a?(Module)] }
show("name") { c.name }
show("ancestors") { c.ancestors == [c] }
show("instance_methods") { c.instance_methods }
show("superclass") { c.superclass }
show("new") { c.new }
show("allocate") { c.allocate }
show("distinct") { Class.allocate.equal?(Class.allocate) }

# The value classes answer their empty value.
show("String") { String.allocate }
show("Array") { Array.allocate }
show("Hash") { Hash.allocate }
show("Object") { Object.allocate.class }
show("Range") { Range.allocate.inspect }

# An exception's payload allocates too, so every `Exception` method still
# reads. A blank message defaults to the class name.
show("RuntimeError") { [RuntimeError.allocate.class, RuntimeError.allocate.message] }
show("Exception") { Exception.allocate.inspect }
show("Errno") { Errno::ENOENT.allocate.class }

# The immediates have no allocator, and a module has no `allocate` at all.
show("Integer") { Integer.allocate }
show("Symbol") { Symbol.allocate }
show("Comparable") { Comparable.allocate }

# `first`/`last` are not `begin`/`end`: an open endpoint HAS no element.
show("beginless first") { (..5).first }
show("beginless begin") { (..5).begin }
show("beginless last") { (..5).last }
show("endless last") { (1..).last }
show("endless end") { (1..).end }
show("endless first") { (1..).first }
show("bounded") { [(1..5).first, (1..5).last, (1...5).last] }
show("n-form") { [(1..5).first(2), (1..5).last(2), (1..).first(2)] }
__END__
[:allocate]
class: Class
is_a: [true, true]
name: nil
ancestors: true
instance_methods: []
superclass: [TypeError, "uninitialized class"]
new: [TypeError, "can't instantiate uninitialized class"]
allocate: [TypeError, "can't instantiate uninitialized class"]
distinct: false
String: ""
Array: []
Hash: {}
Object: Object
Range: "nil..nil"
RuntimeError: [RuntimeError, "RuntimeError"]
Exception: "#<Exception: Exception>"
Errno: Errno::ENOENT
Integer: [TypeError, "allocator undefined for Integer"]
Symbol: [TypeError, "allocator undefined for Symbol"]
Comparable: [NoMethodError, "undefined method 'allocate' for module Comparable"]
beginless first: [RangeError, "cannot get the first element of beginless range"]
beginless begin: nil
beginless last: 5
endless last: [RangeError, "cannot get the last element of endless range"]
endless end: nil
endless first: 1
bounded: [1, 5, 5]
n-form: [[1, 2], [4, 5], [1, 2]]
