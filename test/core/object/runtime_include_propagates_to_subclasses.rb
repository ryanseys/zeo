# A runtime `include` into a superclass must be visible to subclasses minted
# EARLIER (CRuby ancestry is shared structure; zeo chains are flat snapshots
# that the splice now revisits). rspec's runner does exactly this: `describe`
# mints ExampleGroup subclasses, then configure_mock_framework includes the
# mock adapter into ExampleGroup.
class Base
end

module Adapter
  def adapter_says
    "adapter on #{self.class.name || "anon"}"
  end
end

k = Class.new(Base)
Base.include(Adapter)
p k.new.adapter_says
p k.new.is_a?(Adapter)

# Ruby >= 3.0: include into a module ALREADY mixed in elsewhere reaches its
# hosts too.
module Layer
end

class Host
  include Layer
end

module LateAddition
  def late
    "late reached #{self.class}"
  end
end

Layer.include(LateAddition)
p Host.new.late
p Host.ancestors.include?(LateAddition)

# The prepend placement propagates the same way, and shadows the host's own
# definition through a subclass instance.
class PBase
  def who
    "base"
  end
end

sub = Class.new(PBase)
module Shadow
  def who
    "shadow over #{super}"
  end
end
PBase.prepend(Shadow)
p sub.new.who
__END__
"adapter on anon"
true
"late reached Host"
true
"shadow over base"
