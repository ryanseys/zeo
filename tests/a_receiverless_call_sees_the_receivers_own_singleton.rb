# A receiverless call inside a method is a send to `self` like any other, and
# `self` may carry a per-object singleton method that shadows the class's. zeo
# resolved the receiverless form straight to the class's compiled definition,
# so the explicit `self.` spelling and a call from outside saw the singleton
# and the bare name written in the method next door did not.
class Foo
  def close = "class"

  def implicit = close
  def explicit = self.close
end

f = Foo.new
f.define_singleton_method(:close) { "singleton" }
p f.close, f.explicit, f.implicit

# An object WITHOUT the singleton still reaches the class's own method, and the
# two objects disagree at the same call site.
g = Foo.new
p g.close, g.explicit, g.implicit

# The same question asked of a method the class gains at run time, and of one a
# per-object `undef` retires. Both reach the receiverless call through the same
# overlay the singleton does.
class Bar
  def tag = "compiled"
  def read = tag
end
b = Bar.new
p b.read
Bar.define_method(:tag) { "redefined" }
p b.read
b.define_singleton_method(:tag) { "mine" }
p b.read, Bar.new.read

# A singleton method installed with `def obj.name` is the same thing said
# another way.
class Baz
  def kind = "class"
  def ask = kind
end
z = Baz.new
def z.kind = "singleton"
p z.ask, Baz.new.ask
