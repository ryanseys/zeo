# A RUNTIME-defined body does not report its signature. `def obj.m(x, y = 1)`
# and `define_method` both install a proc that knows its own parameters, but
# nothing records them against the METHOD -- so `#arity` falls back to the
# variadic `-1`, `#parameters` is empty, and `#inspect` prints an empty
# parameter list. ruby reports the real signature for both.
def shape(m) = m.inspect.sub(/ [^ ]+:\d+>\z/, ">")

class Widget; end
w = Widget.new
def w.only(x, y = 1) = x
p w.method(:only).arity
p shape(w.method(:only)).sub(/#<Widget:0x[0-9a-f]+>/, "OBJ")
p w.method(:only).parameters

class Gadget
  define_method(:pair) { |a, b = 2, *r| [a, b, r] }
end
p Gadget.instance_method(:pair).arity
p Gadget.instance_method(:pair).parameters
