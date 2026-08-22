# `TracePoint#self` and `#binding` answer for a `:line` event, as CRuby's do:
# `self` is the object the line ran under and `binding` is that scope.
#
# This was a gap. zeo's `Frame` carries a file, a line and a label and nothing
# else -- no receiver, no cell storage -- which is what makes tracing cost one
# relaxed atomic load when no tracepoint is enabled. The receiver slot is
# filled only while a tracepoint is armed, and the binding reuses the capture
# `Kernel#binding` already has, so the cost stays pay-per-use.
#
# The other four readers (`#return_value`, `#parameters`, `#eval_script`,
# `#instruction_sequence`) still refuse on a `:line` event -- so does CRuby.

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
