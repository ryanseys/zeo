# Issue #664: `outer { |x| yield x*2 }` appearing inside an already-
# inlined caller (`inner` itself yields) previously emitted `lv_x` and
# `_block` references in a scope where neither existed — `inner` got
# inlined into main and the codegen left the nested `outer { ... }`
# call site dangling. Fix routes nested user-yield calls through
# compile_nested_yield_call_inline so the inner block's body's yield
# substitutes back to the outer-most caller's block.

def outer
  yield 1
end

def inner
  outer do |x|
    yield x * 2
  end
end

inner { |v| puts v.to_s }

# Multi-arg yield path. inner forwards a 2-tuple, outer's block param
# captures and re-yields one of them. Exercises both args + offsets.
def add_one
  yield 10
end

def double_then_emit
  add_one do |n|
    yield n + 5
  end
end

double_then_emit { |r| puts r.to_s }
