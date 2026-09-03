# A single multi-argument `include`/`prepend` keeps its arguments in SOURCE
# order in the resulting ancestry, which is the OPPOSITE of what separate
# statements produce:
#
#   include A, B      -> [self, A, B]      prepend A, B      -> [A, B, self]
#   include A; include B -> [self, B, A]   prepend A; prepend B -> [B, A, self]
#
# zeo's mro flattens the registered mixin list with a uniform
# "later-registered-is-closer" reversal (correct for the separate-statement
# case), so a single multi-arg statement registers its modules in reverse to
# come out in source order. Verified against ruby for the class-body form, the
# `C.prepend(A, B)` call form, and dispatch through the chain via `super`.

module A
  def who
    "A[" + (defined?(super) ? super : "_") + "]"
  end
end
module B
  def who
    "B[" + (defined?(super) ? super : "_") + "]"
  end
end

class InclBody
  include A, B
  def who
    "InclBody"
  end
end
puts InclBody.ancestors.first(3).inspect
puts InclBody.new.who

class PrepBody
  prepend A, B
  def who
    "PrepBody"
  end
end
puts PrepBody.ancestors.first(3).inspect
puts PrepBody.new.who

class PrepCall
  def who
    "PrepCall"
  end
end
PrepCall.prepend(A, B)
puts PrepCall.ancestors.first(3).inspect
puts PrepCall.new.who

# Separate statements: later one is closest (unchanged by the ordering fix).
class SepPrep
  def who
    "SepPrep"
  end
end
SepPrep.prepend(A)
SepPrep.prepend(B)
puts SepPrep.ancestors.first(3).inspect
puts SepPrep.new.who

class SepIncl
  def who
    "SepIncl"
  end
end
SepIncl.include(A)
SepIncl.include(B)
puts SepIncl.new.who
__END__
[InclBody, A, B]
InclBody
[A, B, PrepBody]
A[B[PrepBody]]
[A, B, PrepCall]
A[B[PrepCall]]
[B, A, SepPrep]
B[A[SepPrep]]
SepIncl
