# `module_function` in a REOPENED module body: only the body recorded as the
# module's definition was scanned, so a module first named by a version file
# and then filled in (the shape every gem has) kept its methods instance-level
# and `M.helper(x)` refused to compile (#3969).
module M
  VERSION = "1"
end
module M
  module_function
  def valid?(x)
    !x.nil?
  end
  def twice(x)
    x * 2
  end
end
module M
  def named(x)
    "n#{x}"
  end
  module_function :named
end
p M.valid?("a")
p M.twice(3)
p M.named(1)
p M::VERSION

module N
  def self.direct = 1
end
module N
  module_function
  def mf = 2
end
p [N.direct, N.mf]

class Includer
  include M
  def call_it = valid?("z")
end
p Includer.new.call_it
