# Method / UnboundMethod — arity, parameters, bind, and bind_call.

class Calc
  def add(a, b); a + b; end
  def scale(x, factor = 2); x * factor; end
  def variadic(first, *rest, last); [first, rest, last]; end
  def keyed(x, y:, z: 0); x + y + z; end
  def nullary; 42; end
end

c = Calc.new

# Bound Method: call, arity, parameters.
m = c.method(:add)
p m.call(3, 4)                 # 7
p m.arity                      # 2
p m.parameters                 # [[:req, :a], [:req, :b]]

p c.method(:scale).arity       # -2 (one optional)
p c.method(:scale).parameters  # [[:req, :x], [:opt, :factor]]
p c.method(:variadic).arity    # -3 (req + rest + post-req)
p c.method(:variadic).parameters
p c.method(:keyed).arity       # 2 (x + required keyword y)
p c.method(:keyed).parameters  # [[:req, :x], [:keyreq, :y], [:key, :z]]
p c.method(:nullary).arity     # 0

# UnboundMethod via Module#instance_method.
um = Calc.instance_method(:add)
p um.class                     # UnboundMethod
p um.name                      # :add
p um.arity                     # 2
bound = um.bind(c)
p bound.class                  # Method
p bound.call(10, 20)           # 30
p um.bind_call(c, 5, 6)        # 11

# Method#unbind is the inverse.
p c.method(:add).unbind.class  # UnboundMethod

# bind rejects an incompatible receiver.
begin
  um.bind("not a Calc")
rescue TypeError
  puts "TypeError"
end
