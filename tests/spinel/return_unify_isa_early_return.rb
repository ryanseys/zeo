# #585 (Sam Ruby). Sibling shape of #581. A method that early-
# returns the param when `value.is_a?(Hash)` is false, then falls
# through to a pointer-shaped value, declared its return type as
# the pointer-shaped path and the early `return value` (int) tried
# to return mrb_int from a pointer-typed function -- fatal under
# -Werror.
#
# The function-signature side surfaces the same widen-to-poly fix
# as #581 (post-fixpoint widen pass). The fix here also closes a
# latent bug in the @unified_imeth_returns marker (spinel's
# Array#index returns -1 for not-found, not nil, so the
# `.index == nil` check at the producer site never added entries
# and the marker stayed empty). Switching to `.include?` lets the
# imeth-family marker get populated, which the widen pass then
# honors -- so the #563 self-operator dispatch family stays intact
# even after the value-vs-pointer widen lands.

class Host
  def stringify_keys(value)
    return value unless value.is_a?(Hash)
    "stringified"
  end
end

# The early-return path returns the int parameter as-is.
puts Host.new.stringify_keys(42)

# The fall-through path returns the pointer-shaped value.
puts Host.new.stringify_keys({"a" => 1})
