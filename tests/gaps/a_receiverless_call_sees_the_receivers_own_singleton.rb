# A receiverless call inside a method is a send to `self`, and `self` may carry
# a per-object singleton method that shadows the class's. zeo resolves the
# receiverless form statically to the class's definition, so only the explicit
# `self.` spelling and an outside call see the singleton.
class Foo
  def close = "class"

  def implicit = close
  def explicit = self.close
end

f = Foo.new
f.define_singleton_method(:close) { "singleton" }
p f.close
p f.explicit
p f.implicit
