# `const_defined?`/`const_get` take an `inherit` flag, and the two answers are
# genuinely different: a top-level constant belongs to `Object`, so every class
# sees it INHERITED and none of them owns it. A block opens no constant scope
# either, so a constant written inside a `Class.new do ... end` body lands at
# top level rather than in the class it appears to build.
def err
  yield
rescue NameError => e
  [:name_error, e.message]
end

TOP = 1
class Compiled
  INSIDE = 2
  class Nested; end
end

p Compiled.const_defined?(:INSIDE, false)
p Compiled.const_defined?(:INSIDE)
p Compiled.const_defined?(:Nested, false)
p Compiled.const_defined?(:TOP, false)
p Compiled.const_defined?(:TOP, true)
p Compiled.const_defined?(:Missing, false)
p Compiled.const_defined?(:Missing)
p Compiled.const_get(:INSIDE, false)
p err { Compiled.const_get(:TOP, false) }
p Compiled.const_get(:TOP, true)

p Object.const_defined?(:TOP, false)
p Object.const_defined?(:Compiled, false)

# A class and a module minted at runtime.
R = Class.new
p R.const_defined?(:TOP, false)
p R.const_defined?(:TOP, true)
M = Module.new
p M.const_defined?(:TOP, false)
p M.const_defined?(:TOP, true)

# A `Class.new do ... end` body is a BLOCK: its constants land at top level.
Outer = Class.new do
  Inner = Class.new do
    def label = "inner"
  end
  X = 5
end
p Inner.new.label
p X
p Outer.const_defined?(:Inner, false)
p Outer.const_defined?(:X, false)
p Object.const_defined?(:Inner, false)
p Object.const_defined?(:X, false)

# An inherited constant is the subclass's by inheritance only.
class Sub < Compiled; end
p Sub.const_defined?(:INSIDE, false)
p Sub.const_defined?(:INSIDE, true)
p Sub.const_get(:INSIDE)

# `const_set` at runtime is visible to both forms.
Compiled.const_set(:LATE, 9)
p Compiled.const_defined?(:LATE, false)
p Compiled.const_get(:LATE)
p Object.const_defined?(:LATE, false)
