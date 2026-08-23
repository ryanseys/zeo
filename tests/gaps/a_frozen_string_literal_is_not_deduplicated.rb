# `"lit".freeze` on a string LITERAL answers the interned fstring in CRuby, so
# two of them are the SAME object. zeo allocates one string per evaluation and
# freezes it, so `equal?` is false.
#
# The rule is a COMPILE-TIME one, not a `String#freeze` one: CRuby's compiler
# turns `<string literal>.freeze` into `opt_str_freeze`, which answers the
# entry in the fstring table. Freezing a string that arrived any other way
# (`s = +"x"; s.freeze`) allocates nothing and dedups nothing, on both engines
# -- so this is about the literal receiver, and nothing else.
#
# zeo already has the table this needs: `String#-@` interns, and `-"lit"` is
# correct here. What is missing is the fold that routes a literal `.freeze`
# into it.
#
# The fix is small and is also a saving -- a frozen string constant is one of
# the most common allocations in a gem -- but it drags a second divergence in
# with it, which is why the two are filed together: CRuby REFUSES a singleton
# class on an fstring (`"lit".freeze.singleton_class` raises `TypeError:
# can't define singleton`), because the object is shared. Interning without
# that refusal would let a program attach a singleton to a string another part
# of the program is also holding, which is worse than the divergence here.

p "lit".freeze.equal?("lit".freeze)
p (-"lit").equal?(-"lit")
p "lit".freeze.frozen?

s = +"built"
s.freeze
p s.equal?("built".freeze)

begin
  "lit".freeze.singleton_class
rescue => e
  p [e.class, e.message]
end

t = +"unshared"
t.freeze
p t.singleton_class.equal?(t.singleton_class)
