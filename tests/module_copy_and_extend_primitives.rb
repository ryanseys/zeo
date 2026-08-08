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

# `undef` through a singleton class retires a name the singleton INHERITS, not
# only one the owner defined: `#<Class:Guarded>`'s ancestors run through
# `Module`, and `rb_undef` resolves with `rb_method_entry`, which walks the
# whole chain. zeo checked the owner's class-METHOD space alone, so the line
# above raised NameError for a method `private_method_defined?` reported on the
# very same receiver.
#
# The tombstone then has to be visible everywhere, because ruby writes it into
# the singleton's own method table where it shadows `Module`'s definition. Each
# of these found that definition again behind the undef:
p Guarded.singleton_class.private_method_defined?(:extend_object)
p (Guarded.singleton_class.instance_method(:extend_object) rescue $!.class)
p Guarded.respond_to?(:extend_object, true)
p (Guarded.send(:extend_object, Object.new) rescue $!.message)

# A PUBLIC inherited name goes the same way, and the owner's own class methods
# and every other module are left alone.
class Klass
  def self.own = :own
end
Klass.singleton_class.send(:undef_method, :name)
p Klass.singleton_class.method_defined?(:name)
p (Klass.name rescue $!.class)
p Klass.own
p String.name
p Module.private_method_defined?(:extend_object)

# `extend_object` really is what `Object#extend` runs.
module Marker
  def marked = "marked"
end

target = Object.new
Marker.send(:extend_object, target)
p target.marked
