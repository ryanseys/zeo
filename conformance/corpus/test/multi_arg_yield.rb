# Multi-argument `yield` in a method body (issue #98).
#
# Pre-fix, every `yield` lowered against a single-arg `_block`
# function pointer (`void (*_block)(mrb_int, void*)`), so any
# `yield x, y[, ...]` emitted `_block(x, y, _benv)` against the
# wrong signature and `cc` rejected it. The yield-inlined path
# (`compile_stmt_with_block`'s YieldNode branch) had a parallel
# bug: smaller-arity yields in a multi-arity method only assigned
# the args that were passed, so unset block-local C vars leaked
# whatever value the previous yield had set.
#
# Fix scans each yield-using method's body for the max yield arity
# and threads that count through `yield_params_suffix*` (so the
# function-pointer signature gets N `mrb_int` slots) and through
# `compile_yield_stmt` (function-pointer-path emit). The inlined
# path is fixed by zeroing remaining `lv_<bp>` slots after each
# yield's args are assigned.

# 1. Basic 2-arg yield.
def add_pair
  yield 1, 2
end

add_pair { |a, b| puts a + b }

# 2. 3-arg yield with block returning to a local.
def sum_triple
  yield 10, 20, 30
end

sum_triple { |a, b, c| puts a + b + c }

# 3. Mixed-arity yields in the same method. The smaller yields
#    must not leak values from the larger ones. Pre-fix, the second
#    yield's `b` would carry 999 from the first yield; post-fix it's
#    0. Test uses `b.to_i` so the CRuby-side `nil` and Spinel-side
#    mrb_int 0 both produce the same numeric value.
def mixed_yield
  yield 100, 999
  yield 200
end

total = 0
mixed_yield { |a, b| total = total + a + b.to_i }
puts total

# 4. Multi-arg yield from a class instance method.
class Dispatcher
  def emit
    yield 100, 200
    yield 300, 400
  end
end

Dispatcher.new.emit { |x, y| puts x + y }

# 5. Three different yield arities in one method. Max-arity detection
#    must find 3 so the function-pointer signature has 3 slots; the
#    smaller yields must zero-pad their unused slots. Test uses
#    `b.to_i` / `c.to_i` so CRuby's `nil` for missing block params
#    and Spinel's `mrb_int 0` produce the same numeric sum.
def varied
  yield 1, 2, 3
  yield 10, 20
  yield 100
end

acc = 0
varied { |a, b, c| acc = acc + a + b.to_i + c.to_i }
puts acc
