# A `dup` override beats universal `Kernel#dup` on both the static String
# site and a Poly site (the any-builtin-overrides fall-through); `clone`
# -- not overridden -- stays the universal shallow copy.

class String
  def dup
    "dupped"
  end
end

s = "orig"
puts s.dup
puts [s].first.dup
puts s.clone
__END__
dupped
dupped
orig
