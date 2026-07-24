# Issue #395 / #392 cluster: inlined yield-bearing method bodies
# previously emitted (a) `self->iv_X` references with no `self` in
# scope at the call site, (b) half-renamed locals (`lv_i_y1` for the
# declaration but `lv_i` for some uses), and (c) `raise X unless Y`
# hoisted out of the unless block.
#
# Fix: compile_yield_method_call_stmt sets @self_override to the
# receiver expression so default ivar emission picks up the right
# pointer. compile_expr_remap learns `[]` / `length` / `size` so
# local-renamed receivers don't fall through to compile_expr.
# fiber_var_ref consults @inline_rename_map_from / _to so any
# CallNode that does fall through still gets the renamed local.
# compile_stmt_with_block gets a UnlessNode arm.

# A) Inlined `each` over an int_array ivar (issue #395 int variant).
class C
  def initialize
    @nums = []
    @nums << 10
    @nums << 20
    @nums << 30
  end

  def each
    i = 0
    while i < @nums.length
      n = @nums[i]
      yield n
      i += 1
    end
    self
  end
end

c = C.new
c.each { |n| puts n.to_s }   # 10 / 20 / 30

# B) `raise X unless Y` inside an inlined yield-method body
# (issue #392).
def measure
  before = 1
  yield
  after = 2
  raise "fail" unless after > before
  true
end

measure { puts "block" }      # block
puts "ok"                     # ok
