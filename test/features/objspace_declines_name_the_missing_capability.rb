# The introspection half that zeo declines: each error names the capability
# it would need, so a caller learns why rather than reading a fabricated
# zero. `tests/objspace_introspection.rb` covers the half that does answer.

require "objspace"
# Each call carries the arguments CRuby's own signature accepts, so
# what it answers is the missing capability rather than an arity error.
{
  memsize_of_all: [], reachable_objects_from_root: [],
  trace_object_allocations_start: [], trace_object_allocations_stop: [],
  dump: ["x"], dump_all: [], dump_shapes: [],
  internal_class_of: ["x"], internal_super_of: ["x"],
}.each do |m, args|
  begin
    ObjectSpace.public_send(m, *args)
  rescue NotImplementedError => e
    puts e.message
  end
end
__END__
ObjectSpace.memsize_of_all needs the allocation registry, which only ZEO_GC=1 arms
ObjectSpace.reachable_objects_from_root is not available (zeo has no GC root table)
ObjectSpace.trace_object_allocations_start is not available (zeo has no allocation hook)
ObjectSpace.trace_object_allocations_stop is not available (zeo has no allocation hook)
ObjectSpace.dump is not available (zeo objects carry no VM header to serialize)
ObjectSpace.dump_all is not available (zeo has no heap enumeration)
ObjectSpace.dump_shapes is not available (zeo has no shape tree)
ObjectSpace.internal_class_of is not available (zeo has no internal classes)
ObjectSpace.internal_super_of is not available (zeo has no internal classes)
