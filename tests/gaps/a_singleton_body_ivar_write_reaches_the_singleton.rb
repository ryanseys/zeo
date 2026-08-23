# An `@x = v` inside `class << obj` writes an ivar of the OBJECT'S SINGLETON
# CLASS, which is a different object from the object. zeo writes it on the
# ENCLOSING self instead, so the value is lost.
#
# The `class << self` form is already right: `map_class_self_items` maps an
# `IvarWrite` to `Item::SingletonIvarWrite` and rebinds it onto
# `self.singleton_class.instance_variable_set`. `desugar_singleton_items`, the
# per-object twin in the same file, has no such arm -- an ivar write names no
# `self`, so it falls to `Item::Passthrough` and runs where it stands.
#
# The read side has the same hole (`Item::SingletonIvarRead` exists only on the
# `class << self` path).
#
# Found by the sweep for `a_singleton_body_call_keeps_the_visibility_barrier_down`,
# which is the OTHER asymmetry between those two arms and is fixed. Both are
# the same shape: a statement the `class << self` path homes onto the singleton
# and the `class << obj` path does not.

I = Module.new
class << I
  @plain = 7
end
p I.singleton_class.instance_variable_get(:@plain)

J = Module.new
class << J
  @counter = 1
  @counter += 1
end
p J.singleton_class.instance_variable_get(:@counter)
