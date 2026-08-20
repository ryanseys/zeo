# Ruby announces a constant the moment it becomes readable: after the
# write, on the module it was set on, once per assignment -- and a class
# or module declaration announces its own name the same way.
module Watched
  def self.const_added(n)
    puts "added #{n}"
  end
  X = 1
  Y = 2
  class Inner; end
  module Nested; end
  Z = X + Y
end
p Watched::X
p Watched::Z

module Late
  A = 1
  def self.const_added(n) = puts("late #{n}")
  B = 2
end
