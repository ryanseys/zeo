# `define_method(name, method_obj)` / `define_singleton_method(name,
# method_obj)` with a `Method`/`UnboundMethod` body (not a Proc): aliases
# within a class, an UnboundMethod copied into a subclass, a bound Method
# as an object singleton, the subclass-compatibility TypeError, and the
# still-working Proc form. Oracle-verified verbatim.

class A
  def greet(n); "hi #{n}"; end
  r = define_method(:hail, instance_method(:greet))
  p r
end
p A.new.hail("x")
class Sub < A
  define_method(:hey, A.instance_method(:greet))
end
p Sub.new.hey("y")
class B < A
  def m(n); n * 2; end
end
class C < B
  define_method(:m2, B.instance_method(:m))
end
p C.new.m2(5)
class Unrelated; end
begin
  Unrelated.class_eval { define_method(:g, A.instance_method(:greet)) }
rescue TypeError => e
  puts e.message
end
class Widget
  def ping(x); "pong #{x}"; end
end
w = Widget.new
w.define_singleton_method(:sm, Widget.new.method(:ping))
p w.sm("z")
class D
  define_method(:sq) { |x| x * x }
end
p D.new.sq(6)
puts "done"
__END__
:hail
"hi x"
"hi y"
10
bind argument must be a subclass of A
"pong z"
36
done
