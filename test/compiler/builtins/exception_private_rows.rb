# `Exception` declares three private rows of its own, not one. zeo had only
# `#initialize`, so `method_missing` and `respond_to_missing?` reported
# `BasicObject` and `Kernel` as their owners and went missing from
# `Exception.private_instance_methods(false)`.
#
# Nothing OBSERVABLE changes about what they do -- the answer is the root pair's
# either way, which is why the divergence survived: the census gates public
# method sets only, so a private row is invisible to it.

p Exception.private_instance_methods(false).sort
p Exception.instance_method(:method_missing).owner
p Exception.instance_method(:respond_to_missing?).owner
p Exception.instance_method(:method_missing).arity
p Exception.instance_method(:respond_to_missing?).arity
p Exception.private_method_defined?(:method_missing)
p Exception.method_defined?(:method_missing)

# Only `Exception` OWNS them, while flat dispatch puts a row on every id -- so
# every descendant must report them private without listing them as its own.
p StandardError.private_instance_methods(false)
p RuntimeError.private_instance_methods(false)
p RuntimeError.private_method_defined?(:method_missing)
p RuntimeError.new.respond_to?(:method_missing)
p RuntimeError.new.respond_to?(:method_missing, true)
p RuntimeError.new.respond_to?(:initialize)

# The behavior is still the root pair's.
class Boom < StandardError; end
begin
  Boom.new("x").nope
rescue NoMethodError => e
  p [e.class, e.message, e.name]
end
p Boom.new("x").respond_to?(:nope)
p Boom.new("x").send(:respond_to_missing?, :nope, true)

# A user override still wins, and its `super` still reaches the root.
class Loud < StandardError
  def method_missing(name, *args)
    return :loud if name == :shout

    super
  end

  def respond_to_missing?(name, include_private = false) = name == :shout || super
end
l = Loud.new("x")
p l.shout
p l.respond_to?(:shout)
p l.respond_to?(:quiet)
begin
  l.quiet
rescue NoMethodError => e
  p e.message
end
__END__
[:initialize, :method_missing, :respond_to_missing?]
Exception
Exception
-1
2
true
false
[]
[]
true
false
true
false
[NoMethodError, "undefined method 'nope' for an instance of Boom", :nope]
false
false
:loud
true
false
"undefined method 'quiet' for an instance of Loud"
