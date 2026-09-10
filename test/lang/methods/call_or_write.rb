# CallOrWriteNode -- `obj.attr ||= val`.
#
# Reads `obj.attr`; if falsy, assigns `obj.attr = val`. Receiver
# evaluated exactly once.
#
# This program covers the don't-fire-when-truthy branch, which is the one
# where the single evaluation is observable. The fire-when-nil branch agrees
# in both.

class Cfg
  attr_accessor :name
  def initialize(n)
    @name = n
  end
end

set = Cfg.new("explicit")
set.name ||= "fallback"   # truthy -> doesn't fire
puts set.name             # explicit

# Receiver evaluated once: a side-effecting receiver only fires once.
$call_count = 0
def get_cfg(c)
  $call_count += 1
  c
end

c = Cfg.new("first")
get_cfg(c).name ||= "second"  # `c.name` is "first" (truthy), so no write,
                              # but get_cfg should still be called exactly once
puts $call_count           # 1
puts c.name                # first
__END__
explicit
1
first
