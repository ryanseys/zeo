# `private` inside a MODULE body did not reach the registry, in any of its
# three forms. The same directives inside a CLASS body worked, which is what
# made this look like a scope-frame problem -- it was not.
#
# A module declares its methods in `own_methods` (the collection that emits the
# `__um_<id>` bridge container), while the registration loop's visibility pass
# reads the materialized `methods` -- EMPTY for a module nothing includes. So
# the mark went nowhere. It is now pushed where the method ROW is pushed, which
# is the one place that can see a module's own methods.

module BareDirective
  def a = 1

  private

  def b = 2
end

module WrappedDef
  def a = 1
  private def b = 2
end

module NamedAfter
  def a = 1
  def b = 2
  private :b
end

module ProtectedDirective
  def a = 1

  protected

  def b = 2
end

class ClassControl
  def a = 1

  private

  def b = 2
end

[BareDirective, WrappedDef, NamedAfter, ProtectedDirective, ClassControl].each do |m|
  puts m.to_s
  p m.instance_methods(false).sort
  p m.private_instance_methods(false).sort
  p m.protected_instance_methods(false).sort
  p [m.private_method_defined?(:b), m.protected_method_defined?(:b), m.method_defined?(:b)]
end

# The mark has to survive into an INCLUDER, which is why declaring it on a
# module matters at all.
class Host
  include BareDirective
end
p Host.new.a
p Host.private_method_defined?(:b)
p Host.new.respond_to?(:b)
p Host.new.respond_to?(:b, true)
begin
  Host.new.b
rescue NoMethodError => e
  puts e.message
end
p Host.new.send(:b)
__END__
BareDirective
[:a]
[:b]
[]
[true, false, false]
WrappedDef
[:a]
[:b]
[]
[true, false, false]
NamedAfter
[:a]
[:b]
[]
[true, false, false]
ProtectedDirective
[:a, :b]
[]
[:b]
[false, true, true]
ClassControl
[:a]
[:b]
[]
[true, false, false]
1
true
false
true
private method 'b' called for an instance of Host
2
