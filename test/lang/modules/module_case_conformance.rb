# GAP -- imported from the spinel corpus at c55d9bdb.
# Module#=== / ancestors ordering includes a module ruby leaves out of the
# match.
#
# Four surfaces around class values and case equality.

# A module has no superclass chain: its ancestors are itself and what it
# includes, not the Object/Kernel/BasicObject a class walks into.
module M; end
module N; include M; end
p M.ancestors
p N.ancestors
class C; include M; end
p C.ancestors
p N.included_modules

# Module#=== is `arg.is_a?(self)`, including for the roots, which fell past
# the arm into the missing-method gate
class U; end
u = U.new
p(Object === u)
p(Object === Object.new)
p(Object === 3)
p(BasicObject === u)
p(U === u)
p(U === 3)
p(Integer === 3)
p(String === "x")

# `when <obj>` is `<obj> === scrutinee`, which for a plain object is its own
# ==; the two pointers were compared instead
class Eq
  def initialize(v); @v = v; end
  def v; @v; end
  def ==(o); v == o.v; end
end

a = Eq.new(1)
b = Eq.new(1)
d = Eq.new(2)
p(a === b)
case b when a then p "eq" else p "ne" end
case d when a then p "eq" else p "ne" end

# a class with no == keeps identity
class Plain; end
p1 = Plain.new
p2 = Plain.new
case p1 when p1 then p "same" else p "other" end
case p2 when p1 then p "same" else p "other" end
__END__
[M]
[N, M]
[C, M, Object, Kernel, BasicObject]
[M]
true
true
true
true
true
false
true
true
true
"eq"
"ne"
"same"
"other"
