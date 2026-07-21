# #700 secondary shape: cast_away_volatile_arg was wrapping a
# coerced call-arg value with the LV's declared type, producing
# `(sp_SymPolyHash *)sp_StrPolyHash_from_sym_poly_hash(lv_form_attrs)`
# -- a SymPoly cast on a StrPoly result.
#
# Requires @needs_setjmp == 1 (any begin/rescue in the file), an
# LV recv with a typed-hash declared type, a callee expecting a
# different hash variant, and the call site to go through
# compile_typed_call_args' compile_expr_for_expected_type +
# cast_away_volatile_arg wrapping.

def takes_str_poly(h)
  h.length
end

def trigger_setjmp
  begin
    raise "x"
  rescue
    1
  end
end

def build_and_call
  form_attrs = {a: "x", b: 1}    # sym_poly_hash
  takes_str_poly(form_attrs)     # callee expects str_poly_hash -> coerce
end

trigger_setjmp                    # arms @needs_setjmp = 1 globally
puts build_and_call
