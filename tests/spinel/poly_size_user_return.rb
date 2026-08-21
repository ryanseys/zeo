# `size` / `length` on a poly receiver answers an Integer -- unless a user class
# owns the name and answers something else, in which case the call is a union.
# The dispatch always emits the builtin length arms (a symbol, a string, every
# array and hash kind), so pinning the call to Integer left their sp_int and
# the user arm's own return meeting in one C slot:
#
#   error: assignment to 'sp_int' from 'sp_PolyArray *'
#
# Only a DISAGREEING user return widens it. A user `size` that answers an
# Integer already agrees with the builtins, and widening every `poly.size`
# would box a count the whole program reads as a number.
class Widget
  def size(extra = nil) = [1, extra]
end
class Counter
  def size = 7
end
def mk(f) = f ? Widget.new : [1, 2]
def mk2(f) = f ? Counter.new : [1, 2]
p mk(true).size
p mk(false).size
p mk2(true).size
p mk2(false).size
p [Widget.new, [1,2]][0].size
p [Widget.new, [1,2]][1].size
p [Counter.new, "ab"][1].size
v = mk(true)
p v.size
p v.size.class
p({a: 1, b: 2}.size)
p [1,2,3].size
p "abc".size
p :sym.size
