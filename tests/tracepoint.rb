# TracePoint: lifecycle and error shapes, then oracle-matched event streams
# over methods, class bodies, blocks, raises, and an unwinding return. Every
# handler filters to this file's own path: CRuby implements enable/disable
# in Ruby, so they inject <internal:trace_point> events zeo's native ones
# never fire. Line numbers ARE the expectation -- re-bless after any edit.
def hum(n)
  t = n * 2
  t + 1
end

def unwind
  raise IndexError, "deep"
end

class Melody
  def notes
    %w[c e g]
  end
  def self.tempo
    120
  end
end

def mine?(t)
  File.basename(t.path) == "tracepoint.rb"
end

# -- lifecycle and error shapes (nothing enabled yet) --
begin
  TracePoint.new(:line)
rescue ArgumentError => e
  p [e.class, e.message]
end
begin
  TracePoint.new(:bogus) { }
rescue ArgumentError => e
  p [e.class, e.message]
end
tp = TracePoint.new(:line) { }
p tp.enabled?
p tp
begin
  tp.lineno
rescue RuntimeError => e
  p [e.class, e.message]
end
p(tp.enable { 42 })
p tp.enabled?
p tp.enable
p tp.enable
p tp.disable
p tp.disable

# -- methods and a class method: call/line/return with attributes --
ev = []
t1 = TracePoint.new(:call, :return, :line) do |t|
  ev << [t.event, t.lineno, t.method_id, t.callee_id, t.defined_class.to_s] if mine?(t)
end
t1.enable
hum(3)
Melody.tempo
t1.disable
ev.each { |e| p e }

# -- inspect inside a handler (path-normalized) and outside --
ins = nil
t2 = TracePoint.new(:call) { |t| ins = t.inspect if mine?(t) }
t2.enable
hum(1)
t2.disable
p ins.sub(/\S*tracepoint\.rb/, "tracepoint.rb")
p t2

# -- a class body: class/end events around its body lines --
ev2 = []
t3 = TracePoint.new(:class, :end, :line) do |t|
  ev2 << [t.event, t.lineno, t.method_id, t.defined_class.to_s] if mine?(t)
end
t3.enable
class Chord
  ROOT = "c"
end
t3.disable
ev2.each { |e| p e }

# -- block iterations refire the block's line --
ev3 = []
t4 = TracePoint.new(:line) { |t| ev3 << [t.lineno, t.method_id] if mine?(t) }
t4.enable
doubled = [10, 20].map { |v| v + 1 }
t4.disable
p doubled
ev3.each { |e| p e }

# -- raise inside a method, unwinding return --
ev4 = []
t5 = TracePoint.new(:call, :return, :raise) do |t|
  if mine?(t)
    rec = t.event == :return ? [t.event, t.method_id] : [t.event, t.lineno, t.method_id]
    rec << [t.raised_exception.class, t.raised_exception.message] if t.event == :raise
    ev4 << rec
  end
end
t5.enable
begin
  unwind
rescue IndexError
end
t5.disable
ev4.each { |e| p e }

# -- raised_exception is only readable during :raise --
err = nil
t6 = TracePoint.new(:call) do |t|
  next unless mine?(t)
  begin
    t.raised_exception
  rescue RuntimeError => e
    err = [e.class, e.message]
  end
end
t6.enable
hum(2)
t6.disable
p err

# -- several tracepoints fire in enable order; trace enables at birth --
order = []
ta = TracePoint.new(:call) { |t| order << [:a, t.method_id] if mine?(t) }
tb = TracePoint.new(:call) { |t| order << [:b, t.method_id] if mine?(t) }
ta.enable
tb.enable
hum(5)
ta.disable
tb.disable
p order
tt = TracePoint.trace(:call) { }
p [tt.class, tt.enabled?]
tt.disable
p tt.enabled?

# -- accessors still fire :call/:return --
# A method whose whole body is one ivar access is normally devirtualized: the
# dispatch table reaches the field with no callee and no frame, so it produces
# no :call and no :return. THIS file names `TracePoint`, which is exactly what
# turns that off (`Hir::uses_call_tracing`), so the events below are the proof
# the gate holds. Appended last on purpose -- every line number above is part
# of the expectation.
class Tune
  attr_accessor :beat

  def initialize
    @beat = 1
  end
end

ev5 = []
t7 = TracePoint.new(:call, :return) do |t|
  ev5 << [t.event, t.method_id, t.defined_class.to_s] if mine?(t)
end
song = Tune.new
t7.enable
song.beat
song.beat = 4
t7.disable
ev5.each { |e| p e }
p song.beat
