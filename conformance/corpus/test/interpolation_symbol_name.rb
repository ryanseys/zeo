# `"#{sym}"` should render the symbol's name, not its int id.
# Pre-fix compile_interpolated had no symbol arm in its
# type-dispatch switch, so the default int path emitted the raw
# `sp_sym` value (`0`, `1`, ...) instead of routing through
# sp_sym_to_s. Issue #633.

s = :hello
puts s
puts "#{s}"
puts s.to_s
puts "x_#{s}_y"

# Multiple symbols + literal mix in one interpolation.
a = :alpha
b = :beta
puts "#{a}-#{b}"

# Symbol from a method return; the call-site infers symbol type
# and the same arm should fire.
def make_sym
  :payload
end
puts "tag=#{make_sym}"
