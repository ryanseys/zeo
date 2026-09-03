# Redefining an operator on the numeric fast-path MRO stands the native
# fast paths down for that operator (per lane), so the user body wins at
# statically-typed sites, poly sites, and send alike -- while untouched
# operators and the other lane keep their native paths (1 + 2, 7 * 6,
# and Integer#+ under a Float-only redefinition stay native).
class Float
  def +(other) = "float-plus(#{self},#{other})"
end
module Comparable
  def <=>(other) = nil
end
p 1.5 + 2.0
p 1 + 2
p 1 + 2.5
x = [1.5].first
p x + 3.0
p 2.5.send(:+, 4.0)
p(-1.5)
p 7 * 6
__END__
"float-plus(1.5,2.0)"
3
3.5
"float-plus(1.5,3.0)"
"float-plus(2.5,4.0)"
-1.5
42
