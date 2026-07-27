# TracePoint is entirely unimplemented -- zeo raises NameError for the bare
# constant instead of supporting execution tracing.
tp = TracePoint.new(:line) { |t| }
tp.enable
tp.disable
p "ok"
