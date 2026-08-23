# Three `ObjectSpace` surfaces zeo still refuses. The rest of the traversal
# family now answers -- this file used to say "there is no allocation
# registry", and there is one.
#
# What works, and is listed below as context rather than as the gap:
# `GC.stat[:total_allocated_objects]` and `[:heap_live_slots]` from the
# registry, `each_object` over `Class`, `Module` and the object kinds the
# registry records, `memsize_of_all` as a fold over that same walk, plus the
# rows that never needed a walk (`count_objects`, `memsize_of`,
# `reachable_objects_from`, `GC.stat[:count]`).
#
# 1. `_id2ref`. `object_id` is a heap ADDRESS here, so the inverse map is not
#    missing so much as unsafe: an address answers only while its value
#    lives, and CRuby's contract is that an id survives long enough to be
#    looked up. The registry could serve the lookup for the kinds it records,
#    which would make `_id2ref` answer for some ids and raise for others --
#    worse than refusing, because the failure would look like a dead object
#    rather than an unsupported kind. It wants an id table of its own, which
#    is also what a stable `object_id` wants.
#
# 2. `dump` / `dump_all`. CRuby emits a JSON record per object with its
#    address, slot size, generation, source file and line. Half those fields
#    describe MRI's heap layout and have no honest zeo answer; the other half
#    is the allocation-tracing record below.
#
# 3. `trace_object_allocations_*`. Every allocation would have to record the
#    Ruby file, line, class path and method that made it. The registry has
#    the hook -- one `record` call per allocation -- but not the data: the
#    compiled allocation sites pass no source position, so this is an
#    emitter change, and it is on the path `GC.stat` is measured against.
#    The `allocation_*` getters already answer nil, which is what CRuby
#    answers for an object allocated before tracing started.
#
# This file runs under `ZEO_GC=1` (see the `.gc` sidecar) so it isolates the
# three refusals. Without it, the registry-backed rows answer nil or refuse
# for a second reason -- the gate, not the mechanism.
#
# Oracle: every row answers.
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
