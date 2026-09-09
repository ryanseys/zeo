# The registry-backed rows here need the allocation registry, which only
# `ZEO_GC=1` arms. Running under it isolates the three surfaces that are
# absent for their OWN reasons rather than for the gate.
#@ zeo-env: ZEO_GC=1
# Three `ObjectSpace` surfaces are refused, and the refusals are decisions
# rather than unbuilt work. The program's other seven rows answer exactly as
# ruby does and are here as context, which is why the file lives in
# `test/divergences/` rather than in `test/gaps/`: a gap that can never flip
# is not work still to do.
#
# 1. `_id2ref`. `object_id` is a heap ADDRESS here, so the inverse map is not
#    missing so much as unsafe: an address answers only while its value lives,
#    and CRuby's contract is that an id survives long enough to be looked up.
#    The allocation registry could serve the lookup for the kinds it records,
#    which would make `_id2ref` answer for some ids and raise for others --
#    worse than refusing, because the failure would read as a dead object
#    rather than an unsupported kind. It wants an id table of its own, which
#    is also what a stable `object_id` wants.
#
# 2. `dump` / `dump_all`. CRuby emits a JSON record per object carrying its
#    address, slot size, generation, source file and line. Half those fields
#    describe MRI's heap layout and have no honest zeo answer; the other half
#    is the allocation-tracing record below.
#
# 3. `trace_object_allocations_*`. Every allocation would have to record the
#    Ruby file, line, class path and method that made it. The registry has the
#    hook -- one `record` call per allocation -- but not the data: the compiled
#    allocation sites pass no source position, so this is an emitter change,
#    and it sits on the path `GC.stat` is measured against. The `allocation_*`
#    getters already answer nil, which is what CRuby answers for an object
#    allocated before tracing started.
#
# Runs under `ZEO_GC=1` (the `.gc` sidecar) so the three refusals are isolated.
# Without the gate the registry-backed rows answer nil or refuse for a second
# reason -- the gate, not the mechanism.
#
# --- ruby 4.0.6 answers ---
# Every row answers `true`; the three refused rows above are where zeo says
# `NotImplementedError` instead.

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
__END__
GC.stat[:count]	true
GC.stat[:total_allocated_objects]	true
GC.stat[:heap_live_slots]	true
ObjectSpace.count_objects	true
ObjectSpace.each_object	true
ObjectSpace.memsize_of	true
ObjectSpace.memsize_of_all	true
ObjectSpace._id2ref	NotImplementedError
ObjectSpace.dump	NotImplementedError
ObjectSpace.trace_object_allocations_start	NotImplementedError
