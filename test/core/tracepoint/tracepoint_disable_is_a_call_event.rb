# CRuby implements TracePoint's own surface in Ruby (<internal:trace_point>),
# so calling one of those methods while a trace is live fires an ordinary
# :call/:return pair -- `tp.disable` reports one last :disable on its way out.
# Line numbers below are trace_point.rb's, ruby 4.0.6.
events = []
tp = TracePoint.new(:call) { |t| events << t.method_id }

def traced_a = 1
def traced_b = traced_a

tp.enable
traced_b
tp.disable
p events

# The pair's two halves land where the gate is open: the `enable` that STARTS
# a trace fires neither half, the `disable` that ends one fires only :call.
seen = []
outer = TracePoint.new(:call, :return) { |t| seen << [t.event, t.method_id, t.path, t.lineno] }
inner = TracePoint.new(:line) { }
outer.enable
inner.enable
inner.enabled?
inner.enable
inner.inspect
TracePoint.stat
inner.disable
outer.disable
seen.each { |e| p e }

# defined_class and self, over both receiver shapes.
who = []
w = TracePoint.new(:call) { |t| who << [t.method_id, t.defined_class.to_s, t.self.class.to_s] }
w.enable
w.enabled?
TracePoint.stat
w.disable
p who

# `to_s` is Object's cfunc in CRuby, not the Ruby `inspect` -- it traces
# nothing and prints the address shape.
t2 = TracePoint.new(:line) { }
p t2.inspect
p(t2.to_s.sub(/0x\h+/, "0xADDR"))
__END__
[:traced_b, :traced_a, :disable]
[:return, :enable, "<internal:trace_point>", 264]
[:call, :enable, "<internal:trace_point>", 261]
[:return, :enable, "<internal:trace_point>", 264]
[:call, :enabled?, "<internal:trace_point>", 306]
[:return, :enabled?, "<internal:trace_point>", 308]
[:call, :enable, "<internal:trace_point>", 261]
[:return, :enable, "<internal:trace_point>", 264]
[:call, :inspect, "<internal:trace_point>", 106]
[:return, :inspect, "<internal:trace_point>", 108]
[:call, :stat, "<internal:trace_point>", 119]
[:return, :stat, "<internal:trace_point>", 121]
[:call, :disable, "<internal:trace_point>", 297]
[:return, :disable, "<internal:trace_point>", 300]
[:call, :disable, "<internal:trace_point>", 297]
[[:enabled?, "TracePoint", "TracePoint"], [:stat, "#<Class:TracePoint>", "Class"], [:disable, "TracePoint", "TracePoint"]]
"#<TracePoint:disabled>"
"#<TracePoint:0xADDR>"
