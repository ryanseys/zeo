# `ObjectSpace`'s introspection half. CRuby ships it as `ext/objspace`, which
# bolts these methods onto the module `gc.c` already defined; zeo has them
# always present, so `require "objspace"` is recognized ceremony.
#
# `memsize_of`'s figures are implementation-defined in CRuby too ("dependent on
# the version and build of the interpreter"), so only the SHAPE of the answer
# is asserted here -- zero for an immediate, growing with the payload.
require "objspace"

p [nil, true, 1, :sym, 1.5].map { |v| ObjectSpace.memsize_of(v) }
p ObjectSpace.memsize_of("hi").positive?
p ObjectSpace.memsize_of("x" * 500) > ObjectSpace.memsize_of("hi")
p ObjectSpace.memsize_of([1, 2, 3]) >= ObjectSpace.memsize_of([])

# `reachable_objects_from` leads with the object's class, then its direct
# references in order, dropping immediates (which aren't heap objects).
p ObjectSpace.reachable_objects_from(nil)
p ObjectSpace.reachable_objects_from(1)
p ObjectSpace.reachable_objects_from(:sym)
p ObjectSpace.reachable_objects_from("hi")
p ObjectSpace.reachable_objects_from([1, "a", :b, "c"])
p ObjectSpace.reachable_objects_from({ "k" => "v", 1 => 2 })
p ObjectSpace.reachable_objects_from(1..5)
# A bignum is the one heap shape whose class isn't a traversed reference.
p ObjectSpace.reachable_objects_from(2**70)

class Widget
  def initialize
    @name = "gear"
    @count = 3
  end
end
p ObjectSpace.reachable_objects_from(Widget.new)

# Every symbol is immortal (nothing ever leaves the table), so the total is
# the one census figure that can be answered.
p ObjectSpace.count_symbols[:immortal_symbol].is_a?(Integer)
p [ObjectSpace.count_nodes, ObjectSpace.count_tdata_objects].map(&:class)

# Untraced objects have no allocation record -- nil, in CRuby as well.
p ObjectSpace.allocation_sourcefile("x")
p ObjectSpace.allocation_sourceline("x")
p ObjectSpace.allocation_class_path("x")
__END__
[0, 0, 0, 0, 0]
true
true
true
nil
nil
nil
[String]
[Array, "a", "c"]
[Hash, "k", "v"]
[Range]
[]
[Widget, "gear"]
true
[Hash, Hash]
nil
nil
nil
