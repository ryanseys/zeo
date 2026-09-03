# `extend M` puts M in the receiver's SINGLETON-class chain -- a `super`
# inside M's method used to panic the compiler ("not found in own
# ancestors"); it now resolves that chain statically (most recent extend
# first), falling back to the runtime walk for builtin defaults.

module Base
  def greet
    "base"
  end
end
module Loud
  def greet
    super + "!"
  end
end
class Host
  extend Base
  extend Loud
end
puts Host.greet
__END__
base!
