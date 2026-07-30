# `set_trace_func` is absent. The EVENT machinery is not the blocker --
# `ext/tracepoint.rs` already fires `:call`/`:return`/`:class`/`:end`/`:line`/
# `:raise`, so a legacy tracer could hang off the same dispatch. The blocker is
# the ARGUMENT list: CRuby calls the hook with
# `(event, file, line, id, binding, classname)`, and this runtime cannot build
# a real Binding at an arbitrary trace point. Passing nil there would break
# every hook that reads it, which is most of what `set_trace_func` is for.
#
# So closing it needs either a Binding constructible from a live frame, or an
# explicit decision to ship the nil-binding form as a documented divergence.
# `c-call`/`c-return` stay impossible either way: a builtin is a Rust fn with
# no frame to fire from.
#
# `trace_var`/`untrace_var` are done and passing -- see `tests/trace_var.rb`.
p defined?(set_trace_func)
p Kernel.private_method_defined?(:set_trace_func)
