# zeo has no way to walk the live heap, so the whole `ObjectSpace` traversal
# surface refuses and two `GC.stat` keys are absent.
#
# There is no allocation registry: objects are `Arc`s handed out by ordinary
# constructors and nothing records them, so `each_object`, `_id2ref`,
# `memsize_of_all`, `reachable_objects_from_root`, `dump_all` and the
# `trace_object_allocations*` family raise `NotImplementedError` naming that
# reality, and `GC.start` only bumps a counter and runs finalizers for
# already-dead weak entries.
#
# The rows that DO work are the ones answerable without a walk:
# `count_objects`, `memsize_of` (one object), `reachable_objects_from` (one
# object's own edges), `GC.stat[:count]`.
#
# This is the same missing mechanism as
# `a_reference_cycle_is_never_reclaimed.rb`, which is why they are one piece
# of work: G14b's stage 0 is an allocation registry -- a thread-local
# segmented arena of `Weak`, filled from nine constructors -- and it is what
# makes BOTH a cycle collector and this surface possible. Its cost lands at
# allocation, which is why stage 0 is a measurement gate before anything is
# built on it.
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
