# Module#instance_method builds an unbound method object; #bind supplies the
# self and the result behaves as an ordinary bound Method (#2676).
class Animal
  def breathe; "inhale"; end
  def speak(x); "say #{x}"; end
end
m = Animal.instance_method(:breathe)
p m.name
p m.arity
p m.owner
a = Animal.new
bm = m.bind(a)
p bm.call
p Animal.instance_method(:speak).bind(a).call("hi")
