module Outer
  X = :outer
end

p Outer.constants.sort
p Outer.const_defined?(:Late)

class Outer::Late
end

p Outer.constants.sort
p Outer.const_defined?(:Late)
