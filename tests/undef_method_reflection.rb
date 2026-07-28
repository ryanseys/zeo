# `undef_method` on a runtime class is a lookup TERMINATOR, not a deletion:
# every probe that walks the MRO has to stop at it rather than answering from
# a still-live ancestor definition.
class Base
  def greet = "hi"
  def self.describe = "Base"
end

Sub = Class.new(Base) do
  undef_method :greet
end

p Sub.new.respond_to?(:greet)
p Sub.new.respond_to?(:greet, true)
p Sub.method_defined?(:greet)
p Sub.public_method_defined?(:greet)
p Sub.private_method_defined?(:greet)
p Sub.instance_methods(false)
p Sub.instance_method(:greet) rescue p $!.class

begin
  Sub.new.greet
rescue NoMethodError
  puts "NoMethodError"
end

# `super` must not reach past the undef either.
Deeper = Class.new(Sub) do
  define_method(:greet) do
    begin
      super()
    rescue NoMethodError
      "no super"
    end
  end
end
p Deeper.new.greet

# Redefining after the undef clears the tombstone.
Sub.class_eval { define_method(:greet) { "back" } }
p Sub.new.greet
p Sub.new.respond_to?(:greet)
p Base.new.greet
