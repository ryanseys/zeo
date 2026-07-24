# Issue #400. `->(a, b) { a + b }` and longer-arity lambdas
# previously emitted broken code: scan_lambda_free_vars saw only
# reqs[0] as a formal param, so the 2nd+ params were treated as
# free variables to capture from the surrounding scope. The
# call site dropped extra args, too.
#
# Fix: scan_lambda_free_vars + compile_lambda_expr collect every
# required param, the lambda fn signature gets one sp_Val* slot
# per param, and compile_lambda_call_expr dispatches to
# sp_lam_call2 / _3 / _4 (new runtime helpers that re-cast the
# stored 1-arg fn ptr to the matching arity at the call site).
# compile_lambda_body_expr learns `*` / `-` / `/` / `%` / `<>=`
# arms so multi-arg arithmetic bodies lower correctly.

add = ->(a, b) { a + b }
puts add.call(3, 4).to_s        # 7

mul3 = ->(a, b, c) { a * b * c }
puts mul3.call(2, 3, 5).to_s    # 30

quad = ->(a, b, c, d) { a + b + c + d }
puts quad.call(1, 2, 3, 4).to_s # 10

# 1-arg still works through the unchanged sp_lam_call path.
double = ->(x) { x * 2 }
puts double.call(21).to_s       # 42

# Comparison
gt = ->(a, b) { a > b }
puts gt.call(5, 3).to_s         # true
puts gt.call(2, 9).to_s         # false
