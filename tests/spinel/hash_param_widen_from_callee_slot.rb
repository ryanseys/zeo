# Hash-typed parameter back-propagation from callee slot:
# `def outer(into); inner(into); end` where `inner` already
# widened its `into` slot to a poly variant (via body
# heterogeneous writes) should widen `outer`'s `into` slot to
# the same poly variant. Symmetric extension of #453's ptr-back-
# prop applied to hash types.
#
# Surfaces in real-blog's
# `CgiIo.parse_form_into(into) -> CgiIo.assign_form_pair(into, ...)`
# chain where assign_form_pair widens to str_poly_hash via its
# body's mixed `into[k] = val` (string) + `into[k] = {}` (hash)
# writes; pre-fix the outer parse_form_into kept `into` typed
# str_int_hash and the C call passed it into a str_poly_hash *
# slot.

module Form
  # Heterogeneous body — into[k] = string AND into[k] = {} —
  # widens `into` to str_poly_hash via narrow_param_hash.
  def self.assign(into, k, v)
    if k.include?("[")
      into[k] = {}
    else
      into[k] = v
    end
  end

  # Bare-call forward of `into` — outer's `into` should widen
  # to the same str_poly_hash via callee-slot back-prop.
  def self.parse(input, into)
    input.split("&").each { |pair| assign(into, pair, "v") }
  end

  # Driver that creates a properly-typed sym_poly_hash and
  # widens through the chain. Using a heterogeneous initial
  # literal pins the LV at str_poly_hash from the start.
  def self.run
    h = { "x" => "v", "nested" => {} }
    parse("a=1&b=2", h)
    h.length
  end
end

puts Form.run
