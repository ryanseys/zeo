# A gem's version file wraps its own definitions in a reload guard, so that
# requiring it twice by two spellings is harmless. `unless defined?(X)` already
# folded; `if !defined?(X)` -- the commoner spelling -- did not, because the
# negation was only ever handed to the weaker of the two guard folders.

if !defined?(Addressable::VERSION)
  module Addressable
    module VERSION
      STRING = "2.9.0"
    end
  end
end
p Addressable::VERSION::STRING

# `not` is the same operator, and the guarded constant may be a plain value.
if not defined?(Excon::VERSION)
  module Excon
    VERSION = "1.2.3"
  end
end
p Excon::VERSION

# It is a REAL guard: a second copy of the same file must not redefine.
if !defined?(Addressable::VERSION)
  module Addressable
    module VERSION
      STRING = "clobbered"
    end
  end
end
p Addressable::VERSION::STRING

# `||` decides as soon as a branch does, the way `&&` already did.
if defined?(Nowhere::AtAll) || !defined?(Kaboom)
  class Kaboom
    def hi = :hi
  end
end
p Kaboom.new.hi

# ... and short-circuits: the right operand is never consulted when the left
# has already settled it.
if !defined?(Kaboom) && defined?(Nowhere::AtAll)
  class NeverBuilt; end
end
p defined?(NeverBuilt)
__END__
"2.9.0"
"1.2.3"
"2.9.0"
:hi
nil
