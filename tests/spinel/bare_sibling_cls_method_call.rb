# Issue #405. A bare call to a sibling `def self.X` inside another
# `def self.Y` body in the same module/class used to emit a
# `cannot resolve call to 'X' on (no receiver) (emitting 0)`
# warning, with the callee's params defaulting to mrb_int (no
# call-site signal reached the inference pass). CRuby treats the
# bare call as `self.X(...)`, which spinel now mirrors via:
#   - infer_type bare-call arm extended to look up
#     `<Class>_cls_<m>` in @meth_* (modules) or @cls_cmeth_*
#     (real classes) when @current_method_name carries the
#     `_cls_` marker (inference) or @current_method_has_self == 0
#     and @current_class_idx pins the owner (emission).
#   - scan_new_calls's CallNode arm widens the sibling's ptypes
#     from the call site's args.
#   - compile_no_recv_call_expr emits the dispatch.

# 1. Module case -- the shape from the issue: outer cmeth makes
# a bare call to a sibling cmeth that takes a string param. The
# bug widened the sibling's param to mrb_int because the bare
# call site contributed no signal.
module Inv
  def self.outer(table)
    inner(table)
    table
  end

  def self.inner(name)
    # If `name` widens to mrb_int (the bug), this str-concat
    # expression would fail to C-compile.
    "rows-for-" + name
  end
end

puts Inv.outer("articles")    # articles
puts Inv.inner("posts")       # rows-for-posts

# 2. Real-class case: sibling cmeth chained through a string
# param. The class path goes through @cls_cmeth_* tables instead
# of the module @meth_* synth-name path; both are handled.
class Greeter
  def self.outer(x)
    inner(x)
  end

  def self.inner(s)
    "hello, " + s
  end
end

puts Greeter.outer("world")   # hello, world

# 3. Multi-hop chain across three sibling cmeths inside a module
# so the iterative inference loop has to propagate types twice.
module Pipeline
  def self.start(s)
    middle(s)
  end

  def self.middle(t)
    finish(t)
  end

  def self.finish(u)
    "[" + u + "]"
  end
end

puts Pipeline.start("ok")     # [ok]
