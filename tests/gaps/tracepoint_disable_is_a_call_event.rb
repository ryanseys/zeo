events = []
tp = TracePoint.new(:call) { |t| events << t.method_id }

def traced_a = 1
def traced_b = traced_a

tp.enable
traced_b
tp.disable
p events
