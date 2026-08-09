# `CONST = begin; require "x"; true; rescue LoadError; end` reads like a
# runtime value and is not one: a whole-program compiler already knows which
# requires it can satisfy. hexapdf gates a whole file on `HARFBUZZ_AVAILABLE`
# spelled exactly this way.
MISSING = begin
            require 'definitely_not_a_real_gem_xyz'
            true
          rescue LoadError
          end
p MISSING
if MISSING
  class ShimA
    def tag = "a"
  end
end
p defined?(ShimA)

PRESENT = begin
            require 'set'
            true
          rescue LoadError
          end
p PRESENT
if PRESENT
  class ShimB
    def tag = "b"
  end
end
p ShimB.new.tag

# `defined?(Scope::NAME)` asks a SCOPED question, so an unrelated `VERSION`
# elsewhere in the program is not an answer to it.
module Other
  VERSION = "9.9"
end
if defined?(NotHere) && defined?(NotHere::VERSION)
  module Adapter
    NOPE = 1
  end
end
p defined?(Adapter)

module Present
  VERSION = "1.0"
end
if defined?(Present::VERSION)
  module Wired
    YES = Present::VERSION
  end
end
p Wired::YES
