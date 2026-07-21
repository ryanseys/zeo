# #490. A method with no reachable caller defaults its params to
# mrb_int, then any operation in the body that's invalid on Int
# fires "cannot resolve call to '<m>' on int (emitting 0)". The
# previous warning omitted the source method and was a debugging
# papercut for library-style entry points: the user had to bisect
# defs to find the culprit. The warning now includes the
# containing method as "in <Class>.<method>" / "in <Class>#<method>"
# / "in <Mod>.<method>" so it's actionable without bisection.
# Test asserts only the program output (the warning goes to
# stderr; the harness drops it via 2>/dev/null), so this test
# also doubles as a "library-method-with-no-caller still compiles"
# regression guard.

module Util
  def self.size_of(xs)
    xs.length
  end
end

puts "hello"
