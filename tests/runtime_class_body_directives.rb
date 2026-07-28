# A class built at runtime runs its body as an ordinary block, so every
# class-body directive has to be rewritten into the equivalent self-send.
module Greeting
  def greet = "hello from #{self.class.name}"
end

module Loud
  def greet = super.upcase
end

Speaker = Class.new do
  include Greeting
  prepend Loud

  def secret = "shh"
  private :secret

  def spoken = greet
  alias_method :say, :spoken
end

p Speaker.new.greet
p Speaker.new.say
p Speaker.private_instance_methods(false).include?(:secret)
p Speaker.ancestors.first(3).map(&:to_s)

module Shout
  def shout(word) = "#{word}!"
end

Helpers = Module.new do
  include Shout
  module_function :shout
end

p Helpers.shout("go")
p Helpers.singleton_methods.sort
p Helpers.private_instance_methods(false)

Trimmed = Class.new(Speaker) do
  undef_method :spoken
end
p Trimmed.new.respond_to?(:spoken)

# A nested class inside a runtime body becomes a runtime class of its own --
# and lands at TOP level, since a block opens no lexical scope for constants.
Outer = Class.new do
  Inner = Class.new do
    def label = "inner"
  end
  def label = "outer"
end
p Outer.new.label
p Inner.new.label
