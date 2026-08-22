# `extend M` written in a class body is a COMPILE-TIME edit, applied before the
# body's first statement runs -- so a `singleton_class.prepend M` written ABOVE
# it in the same body sees an already-extended class where ruby sees a bare
# one, and the two verbs come out in the wrong order.
#
# The rule they disagree about is CRuby's own, and zeo replays it correctly
# everywhere else (`tests/mixins_across_classes_modules_and_singletons.rb`
# sweeps five shapes): `rb_include_module` searches the WHOLE chain
# (`search_super = TRUE`) and `rb_prepend_module` only the PREPEND AREA, so
# whichever verb runs FIRST is the one that finds an empty scope. `extend M`
# then `prepend M` gives the module two singleton positions; `prepend M` then
# `extend M` gives it one, because the extend finds the prepended copy.
#
# Document order is therefore the whole of the answer, and a class-body
# `extend` has none: `class_extends` is baked into the registry and applied at
# class registration. Both RUNTIME spellings are right (`K.extend M` and
# `K.singleton_class.prepend M`, in either order -- oracle-verified), so this
# is exactly the class-body form and nothing else.
#
# It is the shape [[zeo-positional-runtime-flags]] keeps finding: a directive
# the RUN TIME owns, folded into a compile-time set and applied before line 1.
# The fix is to give a class-body `extend` a document position, the way
# `private_constant` and `K.prepend` got one -- a pass of its own, and one that
# should take `include`/`prepend` with it.
#
# BEFORE 2026-08-22 zeo answered THIS shape correctly and `Ordered` below
# wrongly, and it was right for the wrong reason: the singleton chain was built
# with one dedup over the whole concatenation, so it could hold no duplicate at
# all. Every order collapsed to one position, which happens to be this order's
# answer. Building the two areas apart made four shapes right and left this one,
# where the missing fact is document order rather than the search scope.
module M
  def hi = "m"
end

class Reversed
  singleton_class.prepend M
  extend M
end
p Reversed.singleton_class.ancestors.map(&:to_s).first(3)

# The same two verbs written the other way round agree, and so do both
# runtime spellings.
class Ordered
  extend M
  singleton_class.prepend M
end
p Ordered.singleton_class.ancestors.map(&:to_s).first(4)
