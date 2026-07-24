# Issue #351: sister-to-#341. After #341 widened the per-class
# `def [](k)` parameter to `sp_RbVal` (poly) when callers pass
# heterogeneous key types, compile_poly_method_call's per-class
# dispatch arm passed the raw call-site value (`const char *`,
# `mrb_int`, ...) directly to the `sp_RbVal` parameter — gcc
# errored `incompatible type for argument 2 of 'sp_<C>__aref'`.
#
# Fix: at the per-arm emit, when the arm's ptype is `poly` (base
# type) but the call-site arg's type is concrete, wrap the arg
# expression in `box_value_to_poly(arg_type, val)` so the
# call-site value reaches the slot pre-boxed.
#
# Surfaced via Roundhouse's view-helper shape:
# `def field_for(model, field); model[field]; end` where model is
# poly{Article, Comment, HWIA, Parameters} and field is Symbol.

class A
  def [](k)
    "from-A"
  end
end

class B
  def [](k)
    "from-B"
  end
end

# Two callers with different key types — forces the union to
# widen sp_<C>__aref's lv_k to sp_RbVal across A and B.
def lookup_str(receiver, key)
  receiver[key]
end
puts lookup_str(A.new, "alpha")
puts lookup_str(B.new, "beta")

def lookup_sym(receiver, key)
  receiver[key]
end
puts lookup_sym(A.new, :alpha)
puts lookup_sym(B.new, :beta)
