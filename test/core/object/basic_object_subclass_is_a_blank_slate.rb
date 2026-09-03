# Kernel is mixed into Object, so it sits BELOW BasicObject in the chain
# (object.c:4550 -> class.c:1853) and a BasicObject subclass never sees
# it -- the blank slate is pure chain position, not a special case.
# `send`/`public_send` are Kernel's; only `__send__` is BasicObject's
# (vm_eval.c:2960). An absent method must raise even when its result is
# immediately used as a receiver (`a.dup.own`).

class BO < BasicObject
  def initialize; @x = 1; end
  def greet; "hi"; end
  def own; @x; end
end
a = BO.new
r1 = (a.class rescue $!.class); p r1
p a.greet
r2 = (a.inspect rescue "no-inspect"); p r2
r3 = (a.respond_to?(:greet) rescue "no-respond_to"); p r3
r4 = (a.send(:greet) rescue $!.class); p r4
p a.__send__(:greet)
p(a == a)
p(a == BO.new)
p a.equal?(a)
r5 = (a.dup.own rescue $!.class); p r5
p a.own
class Normal; end
p Normal.new.class
p Normal.new.respond_to?(:inspect)
__END__
NoMethodError
"hi"
"no-inspect"
"no-respond_to"
NoMethodError
"hi"
true
false
true
NoMethodError
1
Normal
true
