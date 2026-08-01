# `GC`, `Fiber` and `Binding` rows for facts a refcounted heap with no fiber
# scheduler can state truthfully. Values that describe MRI's own heap (slot
# counts, build options) are asserted by shape; the rest match outright.

puts "== GC =="
# A collection may have run already, so the counters are asserted by TYPE.
puts "count: #{GC.count.is_a?(Integer)}"
puts "total_time: #{GC.total_time.is_a?(Integer)}"
puts "stat: #{GC.stat.class}"
puts "stat_heap: #{GC.stat_heap.class}"
puts "config: #{GC.config.class}"
puts "OPTS: #{GC::OPTS.class}"
puts "INTERNAL_CONSTANTS: #{GC::INTERNAL_CONSTANTS.class}"
# The VALUES describe MRI's last collection; the key set does not.
puts "latest_gc_info keys: #{GC.latest_gc_info.keys.sort.inspect}"
puts "latest_compact_info: #{GC.latest_compact_info.inspect}"
puts "verify_compaction_references: #{GC.verify_compaction_references.class}"
puts "verify_internal_consistency: #{GC.verify_internal_consistency.inspect}"
puts "measure_total_time: #{GC.measure_total_time}"
GC.measure_total_time = false
puts "measure_total_time set: #{GC.measure_total_time}"
GC.measure_total_time = true
puts "auto_compact: #{GC.auto_compact}"
puts "stress: #{GC.stress}"
GC.stress = false
puts "stress set: #{GC.stress}"

puts "== Fiber =="
puts "blocking?: #{Fiber.blocking?}"
puts "current blocking?: #{Fiber.current.blocking?}"
puts "scheduler: #{Fiber.scheduler.inspect}"
puts "current_scheduler: #{Fiber.current_scheduler.inspect}"
puts "set_scheduler nil: #{Fiber.set_scheduler(nil).inspect}"
begin
  Fiber.schedule { 1 }
rescue RuntimeError => e
  puts "schedule: #{e.message}"
end
puts "blocking block: #{Fiber.blocking { |fb| fb.class }}"

f = Fiber.new { Fiber.yield(:paused); :done }
puts "new fiber blocking?: #{f.blocking?}"
puts "inside blocking?: #{Fiber.new { Fiber.blocking? }.resume}"
puts "resume: #{f.resume}"
puts "alive?: #{f.alive?}"
puts "resume again: #{f.resume}"
puts "dead backtrace: #{f.backtrace.inspect}"
puts "dead locations: #{f.backtrace_locations.inspect}"
puts "current backtrace is an Array: #{Fiber.current.backtrace.is_a?(Array)}"

# `Fiber#raise` injects at the yield point.
g = Fiber.new do
  begin
    Fiber.yield(:first)
  rescue ArgumentError => e
    :"caught #{e.message}"
  end
end
puts "raise setup: #{g.resume}"
puts "raise: #{g.raise(ArgumentError, 'boom')}"

puts "== Binding =="
b = binding
puts "implicit_parameters: #{b.implicit_parameters.inspect}"
puts "implicit_parameter_defined?: #{b.implicit_parameter_defined?(:it)}"
begin
  b.implicit_parameter_get(:it)
rescue NameError => e
  puts "implicit_parameter_get: #{e.message.split(' for ').first}"
end
