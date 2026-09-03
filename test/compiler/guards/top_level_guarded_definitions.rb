# A class or module defined inside a top-level `if`.
#
# zeo has to decide such a guard at compile time -- it registers definitions in
# one whole-program walk, so a definition it cannot prove reachable is a
# rejection rather than a runtime branch. `guard_fold` does the deciding, and
# two shapes it could not read cover most of the real gem guards.
#
#   !defined?(X)              a reload guard, and `static_bool` had no `!`
#   "".respond_to?(:m)        a capability probe on a LITERAL receiver, and
#                             `instance_class` mapped only `X.new` / bare `new`
#
# The gems: addressable writes the first; httpclient, em-websocket, highline and
# test-prof write the second.

# `defined?` of a constant the whole program never defines folds false, so the
# negation folds true and the branch is kept.
if !defined?(SomeGemThatIsNotHere)
  class Provided
    def call
      "the guarded definition ran"
    end
  end
end

p Provided.new.call

# `&&` over two of them, which is how a guard that waits on more than one
# optional dependency is written.
if !defined?(AbsentOne) && !defined?(AbsentTwo)
  module Both
    OK = "both absent"
  end
end

p Both::OK

# A capability probe on a string literal. zeo answers `bytesize` from String's
# own native surface, so the guard folds FALSE and the definition never reaches
# codegen -- the native method is what runs.
unless "".respond_to?(:bytesize)
  class String
    def bytesize
      -1
    end
  end
end

p "hello".bytesize
p "héllo".bytesize

# The same probe on the other literal receivers, and through a method the
# receiver inherits (`tap` comes from Kernel, not Array).
unless [].respond_to?(:sum)
  class Array
    def sum
      -1
    end
  end
end

unless({}.respond_to?(:tap))
  class Hash
    def tap
      -1
    end
  end
end

unless 1.respond_to?(:zero?)
  class Integer
    def zero?
      false
    end
  end
end

p [1, 2, 3].sum
p({ a: 1 }.tap { |h| h[:b] = 2 })
p 0.zero?

# `unless !x` -- the double negative folds too, and keeps the branch this time.
unless !defined?(StillNotHere)
  class NeverRuns
    def call
      "unreachable"
    end
  end
end

p defined?(NeverRuns).nil?

# `const_defined?` -- `defined?`'s reflective twin, which guard-compat uses.
unless Object.const_defined?("HostFrameworkIsAbsent")
  module Stubbed
    NAME = "stubbed"
  end
end

p Stubbed::NAME

# A capability probe whose answer is genuinely NO. String's surface is
# projected in full, so the absence is a fact rather than an unfinished class,
# and the branch is kept -- test-prof asks exactly this to detect ActiveSupport.
unless "".respond_to?(:parameterize)
  class Parameterizer
    def call
      "activesupport is not loaded"
    end
  end
end

p Parameterizer.new.call

# A standard stream is an IO instance, which is how highline probes.
unless STDIN.respond_to?(:getbyte)
  class IO
    def getbyte
      -1
    end
  end
end

p STDIN.respond_to?(:getbyte)
__END__
"the guarded definition ran"
"both absent"
5
6
6
{a: 1, b: 2}
true
true
"stubbed"
"activesupport is not loaded"
true
