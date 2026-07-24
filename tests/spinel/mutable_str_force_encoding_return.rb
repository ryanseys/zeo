# Sequel to #441. The original fix gated force_encoding/encode/b
# return-type inference on `recv == "string"`, which left the
# `sp_String *` (mutable_str) receiver case unfixed. Codegen for
# mutable_str dispatches the inner call against `rc + "->data"`,
# so force_encoding's body emits `return lv_out->data` — a
# `const char *`. The function signature still committed to
# mrb_int from the default infer_call_type fallback, tripping
# the C compile with int-from-pointer.
#
# Add the mutable_str arm: return "string" (the body's `->data`
# value type), keeping the C signature aligned with the emitted
# return expression.

def build_via_mutable
  out = String.new
  out << "ab"
  out << "cd"
  out.force_encoding("UTF-8")
end

puts build_via_mutable
