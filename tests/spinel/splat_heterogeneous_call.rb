# Issue #666: heterogeneous call sites against a `def f(*args)` method
# previously left the splat slot at the default `int_array` and packed
# subsequent string args through `(mrb_int)<char*>` casts. Each was
# then re-interpreted by `puts x.to_s` as a giant integer (the pointer
# bit-pattern). Fix widens the splat slot to `poly_array` when call
# sites disagree, and emits a PolyArray builder at the heterogeneous
# call site so each arg is boxed under its concrete type.

def test_splat(*args)
  args.each { |x| puts x.to_s }
end

test_splat(1, 2, 3)
test_splat("a", "b")
