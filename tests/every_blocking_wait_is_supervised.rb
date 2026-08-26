# Every blocking primitive registers with the deadlock detector and parks in
# slices, so none of them can hang where CRuby raises.
#
# Before this, exactly ONE of six waits registered -- `Queue#pop`, which is
# why `a_deadlocked_wait_raises_fatal.rb` passed while `Mutex#lock`,
# `SizedQueue#push` back-pressure, `Thread.stop`, `Thread#join` and both
# Ractor receive sites parked untimed and forever. Registration is now a
# property of the wait primitive rather than something each site remembers.
#
# Two of these are not deadlock verdicts at all, and saying so is the point:
# `Thread.stop` with no other thread is CRuby's `rb_thread_alone()`
# ThreadError, decided BEFORE parking, and joining yourself is a ThreadError
# because there is nobody left to finish the thread being waited on.
#
# DIVERGENCE, recorded rather than fixed: for a mutex deadlock CRuby's
# verdict surfaces in the MAIN thread's `join` frame, while zeo's surfaces
# in the blocked thread's own `Mutex#lock` -- whichever poll comes first.
# The exception, its class and its message agree; only the frame that
# reports it differs, so the rows below read the class and message and the
# mutex row is not here.

def attempt(name)
  r = begin
    yield.inspect
  rescue Exception => e
    # The first line only: CRuby's `fatal` continues with a thread dump of
    # native addresses that no other implementation can produce.
    "#{e.class}: #{e.message.lines.first.chomp}"
  end
  puts "#{name}\t#{r}"
end

# --- Not deadlocks: ThreadError, decided before any wait ------------------
attempt("self join") { Thread.current.join }
attempt("main join") { Thread.main.join }
attempt("stop alone") { Thread.stop }
attempt("recursive lock") do
  m = Mutex.new
  m.lock
  m.lock
end

# --- Real verdicts --------------------------------------------------------
attempt("sized queue back-pressure") do
  q = SizedQueue.new(1)
  q << 1
  q << 2
end

# --- The limit argument, which used to be accepted and ignored ------------
attempt("join(0.05) on a slow thread") do
  t = Thread.new { sleep 5 }
  r = t.join(0.05)
  t.kill
  r
end
attempt("join(0) on a slow thread") do
  t = Thread.new { sleep 5 }
  r = t.join(0)
  t.kill
  r
end
attempt("join(1) on a fast thread") { Thread.new { 1 }.join(1).class }
attempt("join(nil) on a fast thread") { Thread.new { 1 }.join(nil).class }
attempt("join(bad)") { Thread.new { 1 }.join(:x) }

# --- Raising at yourself is delivered NOW, not at some later checkpoint ---
attempt("current raise") { Thread.current.raise(ArgumentError, "now") }
attempt("current raise class only") { Thread.current.raise(TypeError) }

# --- Everything that should still just work -------------------------------
attempt("mutex synchronize") { Mutex.new.synchronize { 42 } }
attempt("uncontended lock") do
  m = Mutex.new
  m.lock
  m.unlock
  m.locked?
end
attempt("contended lock hands over") do
  m = Mutex.new
  out = []
  m.lock
  t = Thread.new { m.lock; out << :second; m.unlock }
  out << :first
  m.unlock
  t.join
  out
end
attempt("value") { Thread.new { 7 }.value }
attempt("stop and wake") do
  t = Thread.new { Thread.stop; :woke }
  sleep 0.01 until t.status == "sleep"
  t.run
  t.value
end
attempt("sized queue drains") do
  q = SizedQueue.new(1)
  consumer = Thread.new { 3.times.map { q.pop } }
  3.times { |i| q << i }
  consumer.value
end
