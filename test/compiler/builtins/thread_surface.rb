# `Thread`'s scheduling, storage and interrupt surface. Values that differ from
# run to run (a native thread id, a backtrace's own paths) are asserted by
# SHAPE, so the file is byte-identical under both engines.

t = Thread.new { Thread.stop }
Thread.pass until t.stop?

# Priority: stored and read back. CRuby's is advisory too.
puts "priority default: #{t.priority}"
t.priority = 3
puts "priority set: #{t.priority}"

puts "stop?: #{t.stop?}"
puts "alive?: #{t.alive?}"
puts "native id: #{t.native_thread_id.is_a?(Integer)}"
puts "pending?: #{t.pending_interrupt?}"

# `#run` wakes a stopped thread and lets it finish.
t.run
t.join
puts "after join alive?: #{t.alive?}"
puts "dead backtrace: #{t.backtrace.inspect}"
puts "dead native id: #{t.native_thread_id.inspect}"

# `wakeup` on a dead thread is a ThreadError.
begin
  t.wakeup
rescue ThreadError => e
  puts "wakeup dead: #{e.message}"
end

# Thread-local storage: `#fetch`'s three-way miss.
me = Thread.current
me[:seen] = 1
puts "fetch hit: #{me.fetch(:seen)}"
puts "fetch default: #{me.fetch(:nope, :fallback)}"
puts "fetch block: #{me.fetch(:nope) { |k| "no #{k}" }}"
begin
  me.fetch(:nope)
rescue KeyError => e
  puts "fetch miss: #{e.message}"
end

# `Thread.start` and `.fork` are `.new` under other names.
puts "start: #{Thread.start(4) { |n| n * 2 }.value}"
puts "fork: #{Thread.fork(5) { |n| n + 1 }.value}"

# `Thread.kill(thr)` is the class spelling of `#kill`.
victim = Thread.new { sleep 30 }
Thread.kill(victim)
victim.join
puts "killed: #{victim.alive?}"

puts "ignore_deadlock: #{Thread.ignore_deadlock}"
Thread.ignore_deadlock = true
puts "ignore_deadlock set: #{Thread.ignore_deadlock}"
Thread.ignore_deadlock = false

puts "abort_on_exception: #{Thread.abort_on_exception}"
puts "pending_interrupt?: #{Thread.pending_interrupt?}"

# The caller's own frames, as objects.
seen = []
Thread.each_caller_location { |loc| seen << loc.class }
puts "each_caller_location: #{seen.uniq.inspect}"

puts "current backtrace: #{me.backtrace.is_a?(Array)}"
puts "current locations: #{me.backtrace_locations.first.class}"

# A trace func is refused rather than accepted and never called.
puts "set_trace_func nil: #{me.set_trace_func(nil).inspect}"
__END__
priority default: 0
priority set: 3
stop?: true
alive?: true
native id: true
pending?: false
after join alive?: false
dead backtrace: nil
dead native id: nil
wakeup dead: killed thread
fetch hit: 1
fetch default: fallback
fetch block: no nope
fetch miss: key not found: :nope
start: 8
fork: 6
killed: false
ignore_deadlock: false
ignore_deadlock set: true
abort_on_exception: false
pending_interrupt?: false
each_caller_location: []
current backtrace: true
current locations: Thread::Backtrace::Location
set_trace_func nil: nil
