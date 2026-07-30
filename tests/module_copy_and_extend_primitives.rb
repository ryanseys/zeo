# Three things `require "delegate"` and `require "singleton"` need, none of
# which worked -- and the first was silently destructive.
#
# `SomeModule.dup` handed back the ORIGINAL. The reason to dup a module is to
# edit the copy, so every such edit hit the original: delegate.rb opens with
# `kernel = ::Kernel.dup` and then undefines six methods on it, which stripped
# `inspect`/`to_s`/`hash`/`===`/`<=>`/`!~` from the real `Kernel` and from
# every object in the program. Nothing raised; reflection just started lying,
# and `Object.instance_method(:inspect)` raised NameError.
module Sample
  def hello = "hello"
  def bye = "bye"
end

copy = Sample.dup
p copy.equal?(Sample)
p copy.instance_methods(false).sort

copy.send(:undef_method, :bye)
p Sample.instance_methods(false).sort
p copy.instance_methods(false).sort

# The load-bearing case, verbatim from delegate.rb.
kernel = ::Kernel.dup
kernel.class_eval do
  [:to_s, :inspect, :!~, :===, :<=>, :hash].each { |m| undef_method m }
end
p Kernel.method_defined?(:inspect)
p Object.instance_method(:inspect).class
p Object.new.inspect.start_with?("#<Object")
p 1.to_s

# `Module#append_features` and `#extend_object` are the primitives `include`
# and `Object#extend` are defined in terms of. They are private, overridable,
# and -- as the singleton gem does -- `undef_method`-able, which needs them to
# exist first.
p Module.private_method_defined?(:append_features)
p Module.private_method_defined?(:extend_object)

module Gatekeeper
  def self.extended(c)
    c.singleton_class.send(:undef_method, :extend_object)
  end
end

module Guarded
  extend Gatekeeper
end

p Guarded.singleton_class.method_defined?(:extend_object)

# `extend_object` really is what `Object#extend` runs.
module Marker
  def marked = "marked"
end

target = Object.new
Marker.send(:extend_object, target)
p target.marked
