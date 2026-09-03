# Exception object surface: value equality (==), identity equality (eql?), the
# #exception copy method (instance and class forms), non-String message
# coercion, and Exception.to_tty?.
e = RuntimeError.new("m")
e2 = RuntimeError.new("m")

# == compares class + message; two distinct RuntimeErrors with the same
# message are equal. eql? is identity (distinct objects are not eql?).
p e == e2
p e == RuntimeError.new("other")
p e == "not an exception"
p e.eql?(e)
p e.eql?(e2)

# #exception with no argument answers self; with a new message, a copy.
p e.exception.equal?(e)
p e.exception("n").message
p e.exception("n").equal?(e)

# The class-method form is an alias for .new.
p RuntimeError.exception("z").message
p RuntimeError.exception("z").class

# A non-String message is coerced to a String via its own to_s.
p RuntimeError.new(42).message
p RuntimeError.new(:sym).message
p ArgumentError.new([1, 2]).message

# Exception.to_tty? answers a boolean.
p [true, false].include?(Exception.to_tty?)

# Raising a non-Exception class is a TypeError, not an attempt to build it.
p(begin; raise String; rescue => ex; ex.class; end)
p(begin; raise Object, "m"; rescue => ex; ex.class; end)
__END__
true
false
false
true
false
true
"n"
false
"z"
RuntimeError
"42"
"sym"
"[1, 2]"
true
TypeError
TypeError
