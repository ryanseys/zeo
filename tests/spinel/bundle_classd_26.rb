# Bundled tests:
#   - poly_nil_return_dispatch
#   - poly_proc_call
#   - poly_recv_bracket_assign

# === poly_nil_return_dispatch ===
# Regression: a subclass override whose static return type is `nil`
# must still be called when dispatched through a poly receiver.
# Previously `box_value_to_poly("nil", call_expr)` dropped `call_expr`
# and emitted a bare `sp_box_nil()` for that arm, so the body of the
# override never ran.
#
# Two checks:
#  1. The override's side effect runs (the `puts` inside T_poly_nil_return_dispatch_Sub#hook).
#  2. The dispatched call's *return value* is correctly observable as
#     nil (i.e. the boxing path threads through, not just the side
#     effect).

class T_poly_nil_return_dispatch_Base
  def hook(arg); end          # empty body -> static return "nil"
end

class T_poly_nil_return_dispatch_Sub < T_poly_nil_return_dispatch_Base
  def hook(arg)
    puts "subclass-ran: " + arg   # `puts` returns nil
  end
end

class T_poly_nil_return_dispatch_Holder
  attr_accessor :h
  def initialize; @h = T_poly_nil_return_dispatch_Base.new; end
  def set(x); @h = x; end
  def call_hook(arg); @h.hook(arg); end
end

h = T_poly_nil_return_dispatch_Holder.new
h.set(T_poly_nil_return_dispatch_Sub.new)
result = h.call_hook("ok")
puts result == nil

# === poly_proc_call ===
# poly_proc_call.rb — verify that .call on a Proc stored in an
# ivar actually invokes the proc body.  Before the fix the dispatch
# body was empty (two separate missing code paths).
# Case 1: Proc stored as sp_RbVal (poly) → emit_poly_builtin_dispatch fix
# Case 2: Proc stored as sp_Proc * (proc) → compile_dot_call_expr fix

class T_poly_proc_call_Runner
  def initialize(pr)
    @pr = pr
  end

  def run
    @pr.call
  end
end

class T_poly_proc_call_Factory
  def wrap(&block)
    block
  end
end

f = T_poly_proc_call_Factory.new

puts "start"
T_poly_proc_call_Runner.new(f.wrap { puts "one" }).run
T_poly_proc_call_Runner.new(f.wrap { puts "two" }).run
puts "end"

# === poly_recv_bracket_assign ===
# `compile_bracket_assign` had no `rt == "poly"` branch. When an
# ivar slot widens to plain `poly` (sp_RbVal — wider than
# poly_array, set by `finalize_ivar_heterogeneity` because of
# multiple distinct non-array writes), `@arr[i] = v` falls through
# every typed branch and emits *nothing* — the assignment silently
# drops from generated T_poly_recv_bracket_assign_C.
#
# Trigger: @arr is observed as int_array (`[nil] * N`) AND as a
# scalar (string, int) — finalize collapses to plain `poly`. Then
# `@arr[i] = v` should still write to the underlying storage, but
# without the poly arm spinel emits zero code for the assignment.

class T_poly_recv_bracket_assign_C
  def init_arr(n)
    @arr = [nil] * n      # int_array observation
    @arr[0] = 100
  end
  def init_str
    @arr = "scalar"       # string observation — widens slot to poly
  end
  def init_int
    @arr = 42             # int observation
  end
  def at(i)
    @arr[i]
  end
end

c = T_poly_recv_bracket_assign_C.new
c.init_arr(3)
puts c.at(0)              # post-write: 100 (master: 0 — write dropped)

