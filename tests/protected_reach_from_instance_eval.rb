# `protected` asks whether the CALLER's `self` is a kind of the method's owner.
# zeo answers that from the class the call SITE sits in -- a compile-time
# constant baked into the site (`codegen::call::visibility::caller_class`,
# recorded in the `CallSite`) -- so anything that changes `self` at RUN time
# without changing the enclosing class is invisible to it.
#
# `instance_eval` and `instance_exec` are the whole population: the block's
# `self` becomes the receiver, so ruby lets it reach that receiver's protected
# methods, while zeo still sees the class the block was WRITTEN in.
#
# Fix shape: the caller class is the only compile-time input to
# `dispatch::explicit_call_barrier`. A block that `instance_eval` re-homes
# would have to carry its runtime `self`'s class to the sites inside it --
# either by passing it (a per-call cost on the hot dynamic path, which is why
# the class rides in the CallSite today) or by compiling such blocks against an
# unknown caller and letting those sites ask at run time.

class Acct
  protected def balance = 42
end

a = Acct.new
b = Acct.new

# The ordinary shape works: the call site sits inside Acct.
class Acct
  def richer?(other) = balance > other.balance
end
p a.richer?(b)

# ...and this one does not, though ruby allows it.
begin
  p a.instance_eval { b.balance }
rescue NoMethodError => e
  puts e.message
end

begin
  p a.instance_exec { b.balance }
rescue NoMethodError => e
  puts e.message
end

# A non-kin `self` is refused either way, which is the half that already agrees.
begin
  p Object.new.instance_eval { b.balance }
rescue NoMethodError => e
  puts e.message
end
