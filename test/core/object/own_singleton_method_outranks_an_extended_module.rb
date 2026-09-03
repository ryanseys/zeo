# CRuby ancestry: a receiver's own `def self.x` beats a module it extends.

module Mixin
  def label; "from mixin"; end
end
module Host
  def self.label; "from host"; end
end
Host.extend(Mixin)
puts Host.label
__END__
from host
