# #581 (Sam Ruby). When a function's explicit `return X` and
# implicit final-expression return disagree on the value-vs-
# pointer axis (here int param early return + hash literal
# implicit), the analyzer's lenient unify_return_type previously
# demoted the int as a default sentinel and picked the hash
# type, declaring the function as `sp_StrStrHash *`. The early
# `return value` then emitted `return mrb_int` from a
# pointer-typed function -- fatal -Wint-conversion under -Werror.
#
# Fix: post-fixpoint pass widens the function's stored return
# type to "poly" when the body's actual return paths contradict
# the analyzer's narrow choice on the value-vs-pointer axis.
# The existing codegen-side `@current_method_return == "poly"`
# boxing arm in compile_return_stmt then routes both branches
# through box_expr_to_poly correctly.
#
# Scope-set-up at the widen-check site (push_scope + declare
# params + refine_method_body_locals) is load-bearing: without
# it, an ivar or local read in the body falls back to a default type and the
# check reports a mismatch for a method whose body is really of one type --
# a reader like `def get(k) = @pool[k]`, say.

def f(value)
  return value if value < 0
  {"k" => "v"}
end

# The early-return path returns the int parameter.
r1 = f(-1)
puts r1.is_a?(Integer)
puts r1

# The implicit-return path returns the hash literal.
r2 = f(5)
puts r2.is_a?(Hash)
puts r2["k"]
__END__
true
-1
true
v
