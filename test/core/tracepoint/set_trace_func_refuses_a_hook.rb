# `set_trace_func` exists and clears, but REFUSES a hook rather than firing it.
#
# The name is now a real row (it used to be absent entirely, which is what this
# file first recorded), so `defined?`, `private_method_defined?` and
# `set_trace_func(nil)` all match ruby. Handing it a Proc raises
# NotImplementedError instead of installing a tracer.
#
# The EVENT machinery is not the blocker -- `ext/tracepoint.rs` already fires
# `:call`/`:return`/`:class`/`:end`/`:line`/`:raise`, so a legacy tracer could
# hang off the same dispatch. The blocker is the ARGUMENT list: CRuby calls the
# hook with `(event, file, line, id, binding, classname)`, and this runtime
# cannot build a real Binding at an arbitrary trace point. Passing nil there
# would break every hook that reads it, which is most of what `set_trace_func`
# is for -- so it refuses loudly instead, the rule `TracePoint.new` already
# follows for events zeo cannot raise.
#
# Closing it needs either a Binding constructible from a live frame, or an
# explicit decision to ship the nil-binding form as a documented divergence.
# `c-call`/`c-return` stay impossible either way: a builtin is a Rust fn with
# no frame to fire from.
#
# `trace_var`/`untrace_var` are done and passing -- see `test/core/tracepoint/trace_var.rb`.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e
  puts "#{label}: #{e.class}: #{e.message}"
end

# These three agree with ruby now.
show("defined?") { defined?(set_trace_func) }
show("private_method_defined?") { Kernel.private_method_defined?(:set_trace_func) }
show("clearing with nil") { set_trace_func(nil) }

# This is what is still missing.
events = []
show("installing a Proc") do
  set_trace_func(proc { |ev, _file, _line, id, _binding, _klass| events << [ev, id] })
  :installed
end
def traced = 1
traced
set_trace_func(nil) rescue nil
show("the hook fired") { events.any? { |ev, id| ev == "call" && id == :traced } }
__END__
defined?: "method"
private_method_defined?: true
clearing with nil: nil
installing a Proc: :installed
the hook fired: true
