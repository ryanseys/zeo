# Real Ractor move semantics: `send(obj, move: true)` hands the graph to the
# receiver and poisons the source -- every later send to a moved object
# raises Ractor::MovedError. (Sibling-duplicate husking and failed-move
# partial poisoning are CRuby traversal accidents zeo does not copy; this
# fixture stays on the well-defined surface.)
$stderr.reopen(IO::NULL) # the experimental warning's file:line is the one nondeterminism

# ---- A moved string: the value arrives, the source refuses everything.
r = Ractor.new { Ractor.receive << "-recv" }
s = +"hello"
r.send(s, move: true)
p r.value
[:length, :==, :inspect, :class, :frozen?, :equal?, :!, :__id__].each do |m|
  begin
    s.__send__(m)
  rescue Ractor::MovedError => e
    p [m, e.class, e.message]
  end
end
begin
  s.instance_eval { 1 }
rescue Ractor::MovedError => e
  p [:instance_eval, e.class, e.message]
end

# ---- Moving a container moves the elements too (single-occurrence graph).
r2 = Ractor.new { Ractor.receive }
inner = +"inner"
arr = [inner]
r2.send(arr, move: true)
p r2.value
p [(inner.length rescue $!.class), (arr.length rescue $!.class)]

# ---- Shareable nodes pass by reference, un-poisoned.
sh = "frozen-shareable".freeze
r3 = Ractor.new { Ractor.receive }
r3.send([sh], move: true)
p [r3.value[0].equal?(sh), sh.length]
r4 = Ractor.new { Ractor.receive }
r4.send(sh, move: true)
p [r4.value.equal?(sh), sh.length]

# ---- Immediates are shareable: nothing to poison.
r5 = Ractor.new { Ractor.receive }
r5.send(42, move: true)
p r5.value

# ---- Unmovable graphs refuse.
r6 = Ractor.new { Ractor.receive }
begin
  r6.send(Thread.current, move: true)
rescue Ractor::Error => e
  p [e.class, e.message]
end
begin
  r6.send(proc {}, move: true)
rescue Ractor::Error => e
  p [e.class, e.message]
end
# ...and the COPY path refuses a proc with CRuby's allocator message.
begin
  r6.send(proc {})
rescue TypeError => e
  p [e.class, e.message]
end
r6.send(:done)
r6.join

# ---- Cycles reconstruct on both paths.
r7 = Ractor.new { Ractor.receive }
c = []
c << c
r7.send(c)
p r7.value[0].equal?(r7.value)
r8 = Ractor.new { Ractor.receive }
c2 = []
c2 << c2
r8.send(c2, move: true)
g8 = r8.value
p g8[0].equal?(g8)

# ---- Plain unfrozen objects: copy leaves the source alone, move poisons it.
class Point
  def initialize
    @x = +"iv"
  end
  attr_reader :x
end
r9 = Ractor.new { Ractor.receive }
pt = Point.new
r9.send(pt)
p [r9.value.class, r9.value.x, pt.x]
r10 = Ractor.new { Ractor.receive }
r10.send(pt, move: true)
g10 = r10.value
p [g10.class, g10.x]
p (pt.x rescue $!.class)

# ---- Hash move: pairs and the default travel.
r11 = Ractor.new { Ractor.receive }
h = { "k" => "v" }
h.default = "d"
r11.send(h, move: true)
p [r11.value, r11.value.default, (h.size rescue $!.class)]

# ---- Port#send takes move: too.
r12 = Ractor.new do
  q = Ractor::Port.new
  Ractor.main.send(q)
  q.receive
end
port = Ractor.receive
ms = +"via-port"
port.send(ms, move: true)
p r12.value
p (ms.length rescue $!.class)

# ---- move: false is the plain copy.
r13 = Ractor.new { Ractor.receive }
s13 = +"still-mine"
r13.send(s13, move: false)
p [r13.value, s13]

# ---- The taxonomy the poison lives in.
p Ractor::MovedError.ancestors[0..3]
p Ractor::MovedObject.superclass
p Ractor::MovedObject.instance_methods(false).sort
p Ractor.shareable?(s) # a husk reports shareable
__END__
"hello-recv"
[:length, Ractor::MovedError, "can not send any methods to a moved object"]
[:==, Ractor::MovedError, "can not send any methods to a moved object"]
[:inspect, Ractor::MovedError, "can not send any methods to a moved object"]
[:class, Ractor::MovedError, "can not send any methods to a moved object"]
[:frozen?, Ractor::MovedError, "can not send any methods to a moved object"]
[:equal?, Ractor::MovedError, "can not send any methods to a moved object"]
[:!, Ractor::MovedError, "can not send any methods to a moved object"]
[:__id__, Ractor::MovedError, "can not send any methods to a moved object"]
[:instance_eval, Ractor::MovedError, "can not send any methods to a moved object"]
["inner"]
[Ractor::MovedError, Ractor::MovedError]
[true, 16]
[true, 16]
42
[Ractor::Error, "can not move Thread object."]
[Ractor::Error, "can not move Proc object."]
[TypeError, "allocator undefined for Proc"]
true
true
[Point, "iv", "iv"]
[Point, "iv"]
Ractor::MovedError
[{"k" => "v"}, "d", Ractor::MovedError]
"via-port"
Ractor::MovedError
["still-mine", "still-mine"]
[Ractor::MovedError, Ractor::Error, RuntimeError, StandardError]
BasicObject
[:!, :!=, :==, :__id__, :__send__, :equal?, :instance_eval, :instance_exec, :method_missing]
true
