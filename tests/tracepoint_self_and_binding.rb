# `TracePoint#self` and `#binding` are DEFINED and refuse loudly, but CRuby
# answers them for a `:line` event: `self` is the object the line ran under
# and `binding` is that scope.
#
# zeo's `Frame` carries a file, a line and a label and nothing else -- no
# receiver, no cell storage -- which is what makes tracing cost one relaxed
# atomic load when no tracepoint is enabled. Fix shape: give `FrameGuard` an
# optional receiver slot filled only while a tracepoint is armed, and reuse
# the `Binding` capture `Kernel#binding` already has (`docs/EVAL_VM.md`), which
# is pay-per-use for the same reason.

seen = []
tp = TracePoint.new(:line) do |t|
  seen << t.self.class
  seen << t.binding.class
  t.disable
end

tp.enable
x = 1
tp.disable

puts seen.inspect
