# Poly-recv hash `.dup`: the call `outer[k].dup` where outer is
# a `*_poly_hash` (and so `outer[k]` returns sp_RbVal) used to
# fall through to int-default ("cannot resolve call to 'dup' on
# int") and silently no-op. Downstream `iv = local` writes
# against typed-hash slots then failed -Wint-conversion.
#
# Two-sided fix:
#  - analyze: infer_call_type for `.dup` recognizes the
#    `nullable_poly_hash[k].dup` shape and returns poly so the
#    receiving LV slot is typed sp_RbVal up front.
#  - codegen: emit_poly_builtin_dispatch grew a `dup` arm that
#    dispatches by cls_id through the matching sp_*Hash_dup
#    runtime helper, boxing the result back as obj.
#
# Surfaces in real-blog's `merged = matched[:path_params].dup`
# in Main.run — 8 cascading warnings (lv_merged stays int, all
# downstream `iv_params = lv_merged` stores warn).

module Router
  def self.match(present)
    if present
      return { controller: :a, path_params: { "id" => "7" } }
    end
    nil
  end
end

# Sink consuming a poly-typed hash. Widens the param to
# sp_RbVal up front via the literal-init call below.
def first_value(h)
  h["id"]
end

# Pin first_value's `h` param to poly via a direct poly-call
# site. This is a workaround for the LV-widening gap: without
# it the param defaults to int because the only call site is
# downstream of the .dup widening.
def make_poly_hash
  hh = { "id" => "fallback" }
  m = Router.match(true)
  if m
    return m[:path_params].dup
  end
  hh
end

x = make_poly_hash
puts first_value(x)
