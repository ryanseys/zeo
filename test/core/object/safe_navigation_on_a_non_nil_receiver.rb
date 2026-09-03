# Only proves `&.` doesn't regress a normal, statically-typed dispatch --
# a receiver that's *actually* nil at runtime needs a nilable/union type
# the compiler's `TyKind` doesn't have yet (every `New` is unconditionally
# a concrete `Object(ClassId)`, never possibly-nil), so that half of
# `&.`'s behavior isn't testable end to end until then.

class Box
  def initialize(value)
    @value = value
  end
  def value
    @value
  end
end

b = Box.new(:present)
puts(b&.value)
__END__
present
