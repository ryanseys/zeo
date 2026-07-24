# Stage 1 of body-side type narrowing (B' direction): if a method's
# parameter is referenced in the body via `param.<m>` where `<m>` is
# defined on exactly one user class (and isn't a common operator /
# built-in-overlap method like `length`/`each`/`+`/...), the param's
# default `int` type narrows to `obj_<C>`.
#
# Without this narrowing the param stayed at `int` until a call-site
# arg-type widened it. For methods that are only ever called via a
# poly receiver (e.g. through `obj.method(:m)` or a heterogeneous
# poly_array dispatch), no concrete call site exists — caller-side
# widening can't fire — and the body would emit raw int operations
# against the param's expected user-class type.

class CPU
  def peek_a(addr); addr * 2 + 1; end
  def cycles_advance(n); n + 100; end
end

# Two separate methods that each take a CPU param. Neither has a
# direct call-site arg whose type spinel statically knows to be CPU
# at the time scan_new_calls runs (caller-side widening), so the
# narrow has to come from the body's method-call shapes.

def fetch(cpu, addr)
  cpu.peek_a(addr)        # peek_a is only on CPU → narrow cpu to obj_CPU
end

def step(cpu, n)
  cpu.cycles_advance(n)   # cycles_advance only on CPU → narrow
end

c = CPU.new
puts fetch(c, 7)          # 15
puts step(c, 3)           # 103
