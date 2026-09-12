# `tp.enable` and `tp.disable` fire only the events ruby's own
# `trace_point.rb` fires around them, in the block form and in the plain
# `enable`/`disable` form.
def m = 1
seen = []
tp = TracePoint.new(:call, :return, :line) { |t| seen << [t.event, t.method_id] }
tp.enable { m }
seen.each { p _1 }
seen.clear
tp = TracePoint.new(:call, :return) { |t| seen << [t.event, t.method_id] }
tp.enable
m
tp.disable
seen.each { p _1 }
__END__
[:line, nil]
[:call, :m]
[:return, :m]
[:return, :enable]
[:call, :m]
[:return, :m]
[:call, :disable]
