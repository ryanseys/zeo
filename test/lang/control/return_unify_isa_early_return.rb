# #585 (Sam Ruby). Sibling shape of #581. A method that early-
# returns the param when `value.is_a?(Hash)` is false, then falls
# through to a pointer-shaped value, declared its return type as
# the pointer-shaped path and the early `return value` (int) tried
# to return mrb_int from a pointer-typed function -- fatal under
# -Werror.
#
# The method's declared answer has to cover both arms, the early return and
# the fall-through alike. A related trap: `Array#index` answers nil for a
# miss and not -1, so an `.index == nil` check never matches where
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
__END__
42
stringified
