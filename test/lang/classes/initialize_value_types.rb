# The value-type and sync-primitive initialize family: for types frozen or
# one-shot by construction, CRuby's refusals ARE the semantics.
def err(label)
  yield
  puts "#{label}: no error"
rescue => e
  puts "#{label}: #{e.class}: #{e.message}"
end

err("range reinit")   { (1..2).send(:initialize, 3, 4) }
err("range icopy")    { (1..2).send(:initialize_copy, (2..9)) }
err("regexp reinit")  { /x/.send(:initialize, "y") }
err("regexp icopy")   { /x/.send(:initialize_copy, /y/) }
err("time reinit")    { Time.now.send(:initialize) }
err("time icopy")     { Time.now.send(:initialize_copy, Time.now) }
err("class reinit")   { String.send(:initialize) }
err("class anon")     { Class.new.send(:initialize) }
err("conv reinit")    { Encoding::Converter.new("UTF-8", "EUC-JP").send(:initialize, "a", "b") }
err("fiber reinit")   { Fiber.new { 1 }.send(:initialize) { 2 } }
err("md icopy type")  { "ab".match(/a/).send(:initialize_copy, 3) }

t = Thread.new { 1 }
t.join
begin
  t.send(:initialize) { 2 }
rescue ThreadError => e
  puts "thread reinit: #{e.class}: #{e.message.split(" - ").first}"
end

# Struct: slot copy between same-class instances only.
S1 = Struct.new(:a, :b)
s = S1.new(1, 2)
p s.send(:initialize_copy, S1.new(8, 9)).equal?(s)
p s
err("struct icopy class") { S1.new(1, 2).send(:initialize_copy, Struct.new(:x).new(5)) }

# Data: frozen by construction, so the hook can only refuse.
D1 = Data.define(:a)
err("data icopy") { D1.new(a: 1).send(:initialize_copy, D1.new(a: 9)) }

# Module: re-runs the body block on ANY module, answers nil.
m = Module.new
p m.send(:initialize) { def hi = 1 }
p m.instance_methods(false)
p Module.new.send(:initialize)
mc = Module.new
p mc.clone.send(:initialize_clone, mc).class

# Sync primitives: permitted no-op / clear / rebound.
mx = Mutex.new
p mx.send(:initialize)
p Mutex.new.freeze.send(:initialize)
q = Queue.new
q << 1
q.send(:initialize)
p q.size
sq = SizedQueue.new(3)
sq.send(:initialize, 5)
p sq.max
p ConditionVariable.new.send(:initialize)

# Random: full-state copy resumes the same stream.
r = Random.new(1)
r.send(:initialize_copy, Random.new(2))
p r.rand(100) == Random.new(2).rand(100)

# Ownership rows.
p [Range, Regexp, Time, Class, Module].map { |c| c.instance_method(:initialize).owner }
p [Range, Regexp, Time, MatchData, Struct, Data].map { |c| c.instance_method(:initialize_copy).owner }
p Module.instance_method(:initialize_clone).owner
__END__
range reinit: FrozenError: can't modify frozen Range: 1..2
range icopy: FrozenError: can't modify frozen Range: 1..2
regexp reinit: FrozenError: can't modify frozen Regexp: /x/
regexp icopy: FrozenError: can't modify frozen Regexp: /x/
time reinit: TypeError: already initialized Time
time icopy: TypeError: already initialized Time
class reinit: TypeError: already initialized class
class anon: TypeError: already initialized class
conv reinit: TypeError: already initialized
fiber reinit: RuntimeError: cannot initialize twice
md icopy type: TypeError: initialize_copy should take same class object
thread reinit: ThreadError: already initialized thread
true
#<struct S1 a=8, b=9>
struct icopy class: TypeError: initialize_copy should take same class object
data icopy: FrozenError: can't modify frozen D1: #<data D1 a=1>
nil
[:hi]
nil
Module
nil
nil
0
5
nil
true
[Range, Regexp, Time, Class, Module]
[Range, Regexp, Time, MatchData, Struct, Data]
Module
