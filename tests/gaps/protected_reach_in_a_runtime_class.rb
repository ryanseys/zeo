# A `protected` method must be reachable from another instance of the same
# class. That works for a compiled `class` body and fails for a body written
# inside `Class.new do ... end`.
#
# `protected` asks whether the CALLER's `self` is a kind of the method's owner,
# and zeo answers it from a class baked into the call site at compile time
# (`codegen::call::visibility`, recorded in the `CallSite`). Inside a
# `Class.new` block the enclosing lexical class is `Object` -- the class the
# body will belong to does not exist until the block runs -- so the site records
# `Object`, which is not a kind of the runtime class, and the call is refused.
#
# `tests/gaps/protected_reach_from_instance_eval.rb` is the sibling: there
# `self` changes at run time without changing the lexical class. Here the
# lexical class is simply not the owner yet. Both want the same thing -- a site
# whose caller class is unknown at compile time must ask at run time.

class Compiled
  def cmp(o) = o.secret
  protected def secret = 7
end
p Compiled.new.cmp(Compiled.new)

Runtime = Class.new do
  def cmp(o) = o.secret
  protected def secret = 8
end
p Runtime.new.cmp(Runtime.new)

# The same through a separate `protected :name` statement.
Later = Class.new do
  def cmp(o) = o.secret
  def secret = 9
  protected :secret
end
p Later.new.cmp(Later.new)
