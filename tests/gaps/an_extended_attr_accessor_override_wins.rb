# A class-body `attr_accessor` must dispatch, not fold, when the program
# overrides it. Ruby looks the name up on the class's singleton chain like any
# other call, so an `extend`ed module's `attr_accessor` wins.
#
# zeo expands a class-body `attr_*` into `DefMethod` nodes at LOWERING, before
# it knows what the class extends, so the override never runs and the builtin
# reader/writer is installed instead -- a silent wrong answer, not an error.
#
# Written four ways: extended directly, extended by an `included` hook (which
# is how net-imap's `Net::IMAP::Config` does it), a no-op override (thor
# disables `attr_reader` in a Thor class this way), and one taking keywords
# the builtin would refuse.
module Macros
  def attr_accessor(name)
    puts "macro attr_accessor #{name}"
    define_method(name) { 42 }
  end
end

class Direct
  extend Macros
  attr_accessor :alpha
end
p Direct.new.alpha
p Direct.instance_methods(false).sort

module ViaHook
  def self.included(mod) = mod.extend(Macros)
end

class Hooked
  include ViaHook
  attr_accessor :beta
end
p Hooked.new.beta
p Hooked.instance_methods(false).sort

module Silencer
  def attr_reader(*) = nil
end

class Silent
  extend Silencer
  attr_reader :gamma
end
p Silent.instance_methods(false).sort

module Typed
  def attr_accessor(name, type: nil)
    define_method(name) { type }
  end
end

class Coerced
  extend Typed
  attr_accessor :delta, type: Integer
end
p Coerced.new.delta
