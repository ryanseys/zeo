# A Method object whose target has a rest parameter takes ONE array there,
# not one C argument per call-site argument. Bound positionally, the first
# argument went into the array pointer's slot and the call died.
class Var
  def sum(*a); a.sum; end
  def one(x); x * 2; end
  def mix(a, *rest); [a, rest]; end
end
p Var.new.method(:sum).call(1, 2, 3)
p Var.new.method(:sum).call
p Var.new.method(:one).call(4)
p Var.new.method(:mix).call(1, 2, 3)
p Var.new.sum(1, 2, 3)
