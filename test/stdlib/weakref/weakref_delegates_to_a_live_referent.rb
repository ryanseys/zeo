require "weakref"
s = "hello"
w = WeakRef.new(s)
p w.class
p w.upcase
p w.__getobj__
p w.weakref_alive?
p w.respond_to?(:upcase)
p w.respond_to?(:no_such_method)
obj = Object.new
def obj.greet(name) = "hi, #{name}"
w2 = WeakRef.new(obj)
p w2.greet("world")
__END__
WeakRef
"HELLO"
"hello"
true
true
false
"hi, world"
