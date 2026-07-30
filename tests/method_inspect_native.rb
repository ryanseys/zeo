# A body installed at RUNTIME reports its own signature. `def obj.m(x, y = 1)`,
# `define_method`, `define_singleton_method`, and a `class_eval`-`def` all
# install a proc that knows its parameters, and `#arity`/`#parameters`/`#inspect`
# each read them back -- keyed by the object for a per-object singleton, so one
# object's `def obj.m` never describes another instance of the same class.
def shape(m) = [m.arity, m.parameters]
def printed(m) = m.inspect.sub(/ [^ ]+:\d+>\z/, ">").sub(/0x[0-9a-f]+/, "ADDR")

class Widget; end
w = Widget.new
def w.only(x, y = 1) = x
p shape(w.method(:only))
p printed(w.method(:only))

w.define_singleton_method(:dsm) { |a, *r, k: 1, &b| }
p shape(w.method(:dsm))
p printed(w.method(:dsm))

# A singleton on a non-object heap value.
A = [1]
def A.pick(i, j = 0) = i
p shape(A.method(:pick))

# A runtime class: both its instance methods and its class methods.
K = Class.new do
  define_method(:pair) { |a, b = 2, *r| }
  def self.cm(q, *s); end
end
p shape(K.instance_method(:pair))
p printed(K.instance_method(:pair))
p shape(K.method(:cm))
p printed(K.method(:cm))

# A compiled class reopened at runtime.
class Reop; end
Reop.define_method(:dm) { |a, b:, **o| }
p shape(Reop.instance_method(:dm))
p printed(Reop.new.method(:dm))
Reop.define_singleton_method(:sdm) { |z = 3| }
p shape(Reop.method(:sdm))

# An override replaces the recorded signature.
Reop.define_method(:dm) { |single| }
p shape(Reop.instance_method(:dm))

class Late; end
Late.class_eval { def ce(u, v = 1); end }
p shape(Late.instance_method(:ce))

# A per-object singleton shadows the class's method of the same name -- for
# that object only.
class Shadow
  def m(a, b); end
end
s = Shadow.new
def s.m(only_one); end
p printed(s.method(:m))
p printed(Shadow.new.method(:m))
p printed(Shadow.instance_method(:m))

# A compiled `def` and a builtin still report what they always did.
class Plain
  def sig(a, b = 1, *c, d:, e: 2, **f, &g); end
end
p shape(Plain.instance_method(:sig))
p [3.method(:between?).arity, [].method(:push).arity]
