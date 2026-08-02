# The top-level `self` renders as `main`, not as an address. ruby gets there
# by installing `to_s`/`inspect` singletons on the object; zeo asks by
# identity, because installing a singleton at startup would mark the
# runtime-overlay maps live for every program.

p self
p self.to_s
p self.inspect
p "#{self}"
p [self.class, self.frozen?]

# Every route to the same object renders the same way.
def top_helper = self
p top_helper
p self.equal?(top_helper)
p [self]
p({ self => 1 }.keys)

# An ivar does not turn it back into the `#<Object:0x... @x=1>` form.
p self.instance_variables
@x = 1
p self.instance_variables
p self

# An ORDINARY Object still renders with its address and ivars.
o = Object.new
p o.to_s.start_with?("#<Object:")
p o.inspect.start_with?("#<Object:")
class Holder
  def initialize = @a = 1
end
p Holder.new.inspect.include?("@a=1")

# `TOPLEVEL_BINDING`'s receiver IS main, and reading the constant is what
# makes zeo materialize the top-level frame at all.
b = TOPLEVEL_BINDING
p b.class
p b.receiver
p b.receiver.equal?(self)
