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
