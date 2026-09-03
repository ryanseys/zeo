# An object's instance variables are reported in ASSIGNMENT order -- by
# `instance_variables` and by the `inspect` built on it. For a class minted at
# run time, zeo reports them in hash order instead.
#
# A compiled class gets a generated struct whose fields are laid out in source
# order, so its objects already answer correctly (the first pair below). A
# `Class.new` instance is a `runtime_meta::DynObject`, whose ivars live in a
# `FMap` -- foldhash, deliberately unordered -- so the order it reports is
# whatever the hash produced, and it is not even stable across ivar names.
#
# Ruby's order is a property of the OBJECT, not the class: an ivar assigned
# later appears later, including one added after construction. That is what
# makes `p obj` stable enough to diff, which is most of what `inspect` is for --
# and a runtime-built class is exactly the kind a fixture or a factory produces.

class Compiled
  def initialize
    @a = 1
    @b = "s"
    @c = :z
  end
end
p Compiled.new.instance_variables
puts Compiled.new.inspect.sub(/0x\h+/, "0xADDR")

Runtime = Class.new do
  def initialize
    @a = 1
    @b = "s"
    @c = :z
  end
end
p Runtime.new.instance_variables
puts Runtime.new.inspect.sub(/0x\h+/, "0xADDR")

r = Runtime.new
r.instance_variable_set(:@d, 4)
p r.instance_variables
__END__
[:@a, :@b, :@c]
#<Compiled:0xADDR @a=1, @b="s", @c=:z>
[:@a, :@b, :@c]
#<Runtime:0xADDR @a=1, @b="s", @c=:z>
[:@a, :@b, :@c, :@d]
