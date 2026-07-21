# CallAndWriteNode -- `obj.attr &&= val`.
#
# Reads `obj.attr`; if truthy, assigns. Mirror of CallOrWriteNode
# with condition inverted. Receiver evaluated exactly once.

class Cfg
  attr_accessor :name
  def initialize(n)
    @name = n
  end
end

set = Cfg.new("hello")
set.name &&= "WORLD"      # truthy -> fires
puts set.name             # WORLD

# Receiver-once verification.
$call_count = 0
def get_cfg(c)
  $call_count += 1
  c
end

c = Cfg.new("first")
get_cfg(c).name &&= "second"  # `c.name` truthy -> fires;
                              # get_cfg called exactly once
puts $call_count           # 1
puts c.name                # second
