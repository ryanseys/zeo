# `!` is an ordinary overridable method in ruby (`BasicObject#!`), and a class
# that defines it decides its own truthiness under `!`. zeo folds `!x` to a
# truthiness test and never dispatches, so the override is ignored.
#
# `not x` is the same send and diverges the same way. A plain truthiness test
# (`x ? a : b`) correctly does NOT call it, so the override changes only the
# explicit operator -- which is why ruby made `!` a real method in 1.9. Null
# object and Maybe-shaped wrappers are the population that notices.
#
# The fold is right for every value that does NOT define `!`, which is nearly
# all of them, so the fix is to decline it exactly where a user body exists --
# the same static gate `emit_mixin_hook` and the definition hooks already use.

class Inverted
  def !() = :bang_called
end

i = Inverted.new
p !i
p (not i)
p !!i

class Blank
  def !() = true
end
b = Blank.new
p !b
p b ? :truthy : :falsy
__END__
:bang_called
:bang_called
false
true
:truthy
