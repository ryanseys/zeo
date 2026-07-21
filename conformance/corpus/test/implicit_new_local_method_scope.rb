# Implicit-new local class resolution must be method-scoped. A local's
# type can never come from a same-named local in a *different* method.
#
# `accept_loop` binds `conn` to a Fiber and forwards it into
# Sched.spawn -> Slot.new (a Fiber-typed constructor). A sibling method
# `handle` binds a same-named `conn` to a Probe -- a class that gets an
# implicit-new specialization (>= 2 zero-arg `.new` sites whose
# initialize seeds an empty-array ivar). With a name-keyed (un-scoped)
# implicit-new-local lookup, accept_loop's Fiber `conn` leaked to the
# Probe type, mis-widening Sched.spawn's `f` param and (at full program
# scale) breaking the generated C. Authoritative repro: OriPekelman/tep
# (Tep::Scheduler.spawn_fiber). This is path coverage, not a
# fails-without-fix minimal repro -- the build break needs a
# fixpoint-ordering asymmetry that does not survive shrinking.
class Slot
  def initialize(f)
    @f = f
  end
  def run
    @f.resume
  end
end

class Probe
  def initialize
    @items = []
  end
  def go
    @items.length
  end
end

class Sched
  def self.spawn(f)
    s = Slot.new(f)
    s.run
  end
end

def run_worker
  f = Fiber.new { 10 }
  Sched.spawn(f)
end

def accept_loop
  conn = Fiber.new { 20 }
  Sched.spawn(conn)
end

def handle
  conn = Probe.new
  conn.go
end

def warmup
  q = Probe.new
  q.go
end

a = run_worker
b = accept_loop
handle
warmup
puts a
puts b
