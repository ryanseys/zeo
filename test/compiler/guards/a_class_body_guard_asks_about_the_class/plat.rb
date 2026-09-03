# The half a later reopen probes: a singleton method and a constant, both
# already installed by the time the reopen's guard runs.
module Gm
  class Plat
    WIDTH = 1

    class << self
      def generic(name) = name
    end
  end
end
