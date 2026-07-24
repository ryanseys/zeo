# Issue #215 / Gemini-review-critical: the receiver expression in
# `<recv>.method(:foo).call(args)` and `<recv>.method(:foo)[args]`
# must be evaluated exactly once. The Method#call lowering reads
# both `iv_fn_ptr` and `iv_self_obj` off the receiver — a naive
# `((cast)rc->iv_fn_ptr)((void *)rc->iv_self_obj, args)` evaluates
# `rc` twice, so a side-effecting receiver fires its side effect
# twice (and pays the cost twice).
#
# This test exercises a Method whose source receiver is the result
# of a side-effecting call. The side-effect counter must increment
# exactly once per `bm.call`, not twice.

class Worker
  def initialize(base)
    @base = base
  end
  def shout(x)
    @base + x
  end
end

$count = 0

def make_worker
  $count = $count + 1
  Worker.new(100)
end

# Chained: the receiver of `.call` is the whole make_worker.method(...)
# expression. Double-eval would fire the side effect twice.
puts make_worker.method(:shout).call(7)   # 107
puts $count                                # 1

# Same shape via Method#[].
$count = 0
puts make_worker.method(:shout)[42]        # 142
puts $count                                # 1
