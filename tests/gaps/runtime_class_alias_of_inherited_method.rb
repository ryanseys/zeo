# `alias_method` for an INHERITED method, inside a `Class.new` body, aborts the
# process -- "class_id guarantees this downcast" -- instead of answering.
#
# The alias copies the resolved entry for `greet` onto the new class. That entry
# is `Base`'s compiled method, whose body downcasts its receiver to `Base`'s
# generated struct (zeo materializes a layout-correct method per class, so
# `Base#greet` cannot run on another class's instance). A runtime `Class.new`
# subclass's instances are `runtime_meta::DynObject`s, not `Base` structs, so
# the downcast fails and the `expect` fires.
#
# It is the same shape as the `Method`/`UnboundMethod` snapshot bug fixed in
# `tests/method_object_freezes_its_entry.rb`: what must be copied is the LAYER
# the name resolved through, re-resolved per receiver, never the resolved body.
# A compiled `class Sub < Base` aliasing the same name is fine -- materialization
# gives Sub its own copy -- so only the runtime-minted class reaches this.

class Base
  def greet = "base"
end

Sub = Class.new(Base) do
  alias_method :old_greet, :greet
  def greet = "new+" + old_greet
end

p Sub.new.greet
p Sub.new.old_greet
p Sub.instance_method(:old_greet).owner
