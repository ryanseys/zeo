# Top-level `include <Mod>` mixes the module's module_function
# methods into the main scope so bare `<m>` calls dispatch to
# them. CRuby installs them on Object; Spinel registers parallel
# bare-name entries in @meth_* pointing at the same body_id.

module Greeter
  module_function

  def hello(name)
    "hello, " + name + "!"
  end

  def bye
    "bye"
  end
end

include Greeter

puts hello("world")
puts bye

# ---
# `include Outer::Inner` (ConstantPathNode arg) at top level.

module Outer
  module Inner
    module_function

    def greet
      "hi-from-Outer-Inner"
    end
  end
end

include Outer::Inner

puts greet

# ---
# Multi-include with overlap: a later include shadows the earlier
# alias for the same bare name (CRuby: last-include wins via the
# linearised ancestor chain). Non-overlapping methods from both
# modules stay available.

module First
  module_function

  def shared
    "shared-from-First"
  end

  def only_in_first
    "only-First"
  end
end

module Second
  module_function

  def shared
    "shared-from-Second"
  end

  def only_in_second
    "only-Second"
  end
end

include First
include Second

puts shared           # Second wins
puts only_in_first    # still resolvable via First
puts only_in_second   # newly available via Second

# ---
# A user-defined top-level `def` is not shadowed by a later
# `include` of a module with the same method name. CRuby's
# method-lookup places user defs above the include chain.

module Tail
  module_function

  def kept_for_user
    "from-Tail"
  end

  def tail_only
    "tail-only"
  end
end

def kept_for_user
  "user-defined"
end

include Tail

puts kept_for_user   # user-defined wins
puts tail_only       # still resolvable
