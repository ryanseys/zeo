# The `ObjectSpace` traversal family. Seven rows answer exactly as ruby
# does; three -- `_id2ref`, `dump` and `trace_object_allocations_*` -- are
# refused on purpose, and the `.divergence` sidecar says why.
#
# Runs under `ZEO_GC=1` (the `.gc` sidecar) so the refusals are isolated:
# without the gate the registry-backed rows answer nil or refuse for a
# second reason -- the gate, not the mechanism.
require "objspace"
probes = {
  "GC.stat[:count]" => -> { GC.stat[:count].is_a?(Integer) },
  "GC.stat[:total_allocated_objects]" => -> { GC.stat[:total_allocated_objects].is_a?(Integer) },
  "GC.stat[:heap_live_slots]" => -> { GC.stat[:heap_live_slots].is_a?(Integer) },
  "ObjectSpace.count_objects" => -> { ObjectSpace.count_objects.is_a?(Hash) },
  "ObjectSpace.each_object" => -> { ObjectSpace.each_object(Class) { break }; true },
  "ObjectSpace.memsize_of" => -> { ObjectSpace.memsize_of("x").is_a?(Integer) },
  "ObjectSpace.memsize_of_all" => -> { ObjectSpace.memsize_of_all.is_a?(Integer) },
  "ObjectSpace._id2ref" => -> { ObjectSpace._id2ref(1); true },
  "ObjectSpace.dump" => -> { ObjectSpace.dump(Object.new).is_a?(String) },
  "ObjectSpace.trace_object_allocations_start" => -> { ObjectSpace.trace_object_allocations_start; true },
}
probes.each do |name, fn|
  r = begin
    fn.call
  rescue Exception => e
    e.class.to_s
  end
  puts "#{name}\t#{r}"
end
