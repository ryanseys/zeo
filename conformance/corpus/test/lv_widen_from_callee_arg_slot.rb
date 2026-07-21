# Phase 3: LV back-propagation from callee arg slot.
#
# Body-derived widening of a callee's param (via
# narrow_param_hash_types_from_body_writes) doesn't reach
# the caller's local variable. The call site then passes
# sp_StrIntHash * into a sp_StrPolyHash * slot.
#
# Shape (real-blog parse_request / parse_form_into):
#
#   def assign(into, k, v)
#     # heterogeneous body — widens `into` to str_poly_hash
#     if k.include?("[")
#       into[k] = {}
#     else
#       into[k] = v
#     end
#   end
#
#   def parse(input, into)
#     input.split("&").each { |p| assign(into, p, "v") }
#   end
#
#   def run
#     params = {}                     # str_int_hash by default
#     parse("a=1&b=2", params)        # callee widens; params LV
#                                     # needs to widen too
#     params
#   end
#
# Pre-fix the `params = {}` literal compiles as
# sp_StrIntHash_new() and the call passes it to the
# StrPolyHash slot — warns under -Wincompatible-pointer-types.
# With this pass, params widens to str_poly_hash and the
# literal lowers to sp_StrPolyHash_new() directly.

module Form
  def self.assign(into, k, v)
    if k.include?("[")
      into[k] = {}
    else
      into[k] = v
    end
  end

  def self.parse(input, into)
    input.split("&").each { |p| assign(into, p, "v") }
  end

  def self.run
    params = {}
    parse("a=1&b=2", params)
    params["a"]
  end
end

puts Form.run
