# `Foo.method(:a).unbind.bind(Foo)` raises, where ruby allows it.
#
# A class method's `UnboundMethod` is owned by `#<Class:Foo>`, and `Foo` IS an
# instance of that singleton class -- so the bind is legal. zeo now reports the
# right OWNER (tests/issue_class_method_owner.rb), but `bind`'s type check
# still compares the argument against the UnboundMethod's `home`, which for a
# singleton lookup is the class itself. `Foo` is not an instance of `Foo`, so
# it refuses.
#
# Fix shape: `bind` should check against what `#owner` now answers -- the
# singleton class for a `MethodKind::Singleton` unbind -- rather than `home`.
# The instance case is unaffected, where the two are the same class.

class Foo
  def self.a = 1
  def b = 2
end

p Foo.method(:a).unbind.owner
p Foo.method(:a).unbind.bind(Foo).call

# The INSTANCE case already works, which is what makes this the singleton
# branch rather than `bind` as a whole.
p Foo.instance_method(:b).bind(Foo.new).call

# ...and a genuinely wrong argument must still be refused.
begin
  Foo.instance_method(:b).bind(Object.new)
rescue TypeError => e
  puts e.message
end
