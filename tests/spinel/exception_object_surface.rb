# Exception object methods: ==/!=/eql?, dup/clone, #exception, non-String
# message coercion, Exception.to_tty?, raise of a non-Exception class.
e = RuntimeError.new("m")
e2 = RuntimeError.new("m")
p e == e2
p e != e2
p e == RuntimeError.new("other")
p e == "not an exception"
p e.eql?(e)
p e.eql?(e2)
p e.eql?(42)
d = e.dup
p d.equal?(e)
p d.message
c = e.clone
p c.equal?(e)
p e.exception.equal?(e)
p e.exception("n").message
p e.exception("n").equal?(e)
p RuntimeError.exception("z").message
p RuntimeError.exception("z").class
p RuntimeError.new(42).message
p RuntimeError.new(:sym).message
p ArgumentError.new([1, 2]).message
p [true, false].include?(Exception.to_tty?)
r = (raise String rescue $!.class)
p r
r2 = (raise Object, "m" rescue $!.class)
p r2
begin; raise ArgumentError, 42; rescue => ex; p ex.message; end
class E1 < StandardError
  attr_accessor :x
end
a = E1.new("m"); a.x = 1
b = a.dup
b.x = 2
p [a.x, b.x]
