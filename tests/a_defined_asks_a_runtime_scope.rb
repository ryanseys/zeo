p defined?(Nope)
p defined?(Nope::Deep)
p defined?(Nope::Deep::Deeper)
Scoped = Module.new do
  X = 1
end
p defined?(Scoped::X)
p defined?(Scoped::Y)
Val = 7
p defined?(Val::Anything)
p defined?(::Nope::Deep)
module Outer; end
p defined?(Outer::Missing)
S2 = Module.new
S2.const_set(:Z, 5)
p defined?(S2::Z)
p S2::Z
Num = 3
p defined?(Num::Anything)
p defined?(Nope) ? "yes" : "no"
