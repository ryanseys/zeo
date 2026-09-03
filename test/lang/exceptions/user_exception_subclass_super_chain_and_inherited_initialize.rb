# A user exception subclass with NO `initialize` inherits the native default:
# `Plain.new(msg)` stores the message in the hidden slot, so `#message`
# returns it and `instance_variables` is empty. Multi-level `super` chains
# (`B < A < StandardError`, bare `super` forwarding) also resolve natively.

class Plain < RuntimeError
end
p1 = Plain.new("hi")
puts p1.message
p p1.instance_variables

class A < StandardError
  def initialize(msg = "a-default")
    super
  end
end
class B < A
  def initialize
    super()
    @tag = "b"
  end
  def tag; @tag; end
end
b = B.new
puts b.message
puts b.tag
puts b.is_a?(A)

class Done < StopIteration
end
puts Done.new("stop").message
__END__
hi
[]
a-default
b
true
stop
