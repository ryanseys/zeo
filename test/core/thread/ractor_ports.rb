# The ruby 4 Ractor port model: Ractor::Port primitives, default-port
# routing, select, monitor/join/value, locals, and the registry queries.
$stderr.reopen(IO::NULL) # the experimental warning's file:line is the one nondeterminism

# ---- Port primitives, inside a ractor (only the creator receives).
r = Ractor.new do
  q = Ractor::Port.new
  p q.closed?
  p (q << 1).equal?(q)
  q.send(2)
  p [q.receive, q.receive]
  q.close
  p q.closed?
  begin
    q.receive
  rescue Ractor::ClosedError => err
    p [err.class, err.message]
  end
  :done
end
p r.value

# ---- default_port routing: Ractor#send == default_port.send.
r2 = Ractor.new do
  a = Ractor.receive
  b = Ractor.receive
  [a, b, Ractor.current.default_port.class]
end
p r2.send(:x).equal?(r2)
r2 << :y
p r2.value

# ---- select over ports and ractors.
r3 = Ractor.new { :from_r3 }
got = Ractor.select(r3)
p [got[0].equal?(r3), got[1]]

# ---- monitor/unmonitor tokens.
port = Ractor::Port.new
r4 = Ractor.new { :ok }
r4.join
p r4.monitor(port)   # already terminated -> false, token sent
p port.receive
r5 = Ractor.new { Ractor.receive }
p r5.monitor(port)
p r5.unmonitor(port).equal?(r5)
r5.send(:go)
r5.join

# ---- value/join, the successor rule, RemoteError.
r6 = Ractor.new { 6 * 7 }
p r6.value
p r6.value  # same ractor may re-take
r7 = Ractor.new { raise "boom" }
begin
  r7.value
rescue Ractor::RemoteError => e
  p [e.class, e.message, e.ractor.equal?(r7), e.cause.class, e.cause.message]
end

# ---- locals.
Ractor[:k] = 1
p Ractor[:k]
p Ractor.current[:k]
Ractor.current[:k] = 2
p Ractor[:k]
begin
  r6[:k]
rescue RuntimeError => e
  puts e.message
end
p Ractor.store_if_absent(:memo) { 10 }
p Ractor.store_if_absent(:memo) { 20 }

# ---- registry queries and identity.
p Ractor.main?
p Ractor.current.equal?(Ractor.main)
p Ractor.count >= 1
p Ractor.main.inspect.sub(/#\d+/, "#N")
r8 = Ractor.new { :x }
r8.join
p r8.inspect.sub(/#\d+/, "#N").sub(/ \S+\.rb:\d+/, " LOC")

# ---- shareable_proc enforces self-shareability.
pr = Ractor.shareable_proc { 5 }
p [pr.call, pr.frozen?]
__END__
false
true
[1, 2]
true
[Ractor::ClosedError, "The port was already closed"]
:done
true
[:x, :y, Ractor::Port]
[true, :from_r3]
false
:exited
true
true
42
42
[Ractor::RemoteError, "thrown by remote Ractor.", true, RuntimeError, "boom"]
1
1
2
Cannot get ractor local storage for non-current ractor
10
10
true
true
true
"#<Ractor:#N running>"
"#<Ractor:#N LOC terminated>"
[5, true]
