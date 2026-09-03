# A `Class.new(Base)` where `Base` is a COMPILED class cannot run any method
# it inherits: the process aborts with "class_id guarantees this downcast".
# `alias_method` is only the shape that reaches it first.
#
# zeo compiles `Base#greet` into an inherent method on a generated `Base`
# struct, reached through a trampoline that downcasts the receiver to that
# struct. A compiled `class Sub < Base` is safe because `analyze::mro`
# materializes a layout-correct copy of every inherited method onto `Sub` at
# compile time. `Class.new(Base)` mints its class at RUN time, so there is no
# copy to materialize: `runtime_class_new` gives it a `DynObject` -- name-keyed
# ivars, no struct -- and the ancestor walk hands that object to `Base`'s
# trampoline, which downcasts and aborts.
#
# So this is not the `Method`/`UnboundMethod` snapshot bug that
# `tests/method_object_freezes_its_entry.rb` fixed, and copying the LAYER
# rather than the body does not close it: re-resolving through the layer finds
# the same struct-downcasting trampoline. What is missing is the bridge
# `builtins/value_subclass.rs` already builds for a builtin root -- an instance
# that answers the runtime subclass for `.class` while carrying the compiled
# ancestor's own payload for its methods to run against. `Class.new` over a
# builtin root (`Class.new(String)`) works today for exactly that reason.
#
# A body written inside the `Class.new` block is unaffected: those become
# runtime overlay entries, which take their receiver as an argument.

class Base
  def greet = "base"
  def with_ivar
    @x = 1
    "iv#{@x}"
  end
end

# The plain case -- no alias anywhere.
Plain = Class.new(Base)
p Plain.new.greet
p Plain.new.with_ivar

# The alias case, which is how this was first found.
Sub = Class.new(Base) do
  alias_method :old_greet, :greet
  def greet = "new+" + old_greet
end

p Sub.new.greet
p Sub.new.old_greet
p Sub.instance_method(:old_greet).owner

# A builtin root already bridges, and must keep working.
Tagged = Class.new(String)
p Tagged.new("hi").upcase
p Tagged.new("hi").class == Tagged
__END__
"base"
"iv1"
"new+base"
"base"
Sub
"HI"
true
