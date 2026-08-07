# `private :inherited_method` does not redefine anything -- it binds the name on
# THIS class at a new visibility, still running the ancestor's body. Ruby plants
# a real method entry for it (CRuby's VM_METHOD_TYPE_ZSUPER), which is why the
# name then answers `private_instance_methods(false)` and
# `instance_method(:x).owner` on the subclass.

class Base
  def visible = :visible
  def also = :also
  def hidden = :hidden
  private :hidden
end

class Sub < Base
  private :visible
end

class Reopened < Base
  public :hidden
end

p Sub.instance_methods(false).sort
p Sub.private_instance_methods(false).sort
p Sub.instance_method(:visible).owner
p Sub.new.method(:also).owner

# The call site has to see the SUBCLASS's answer, not the definition's.
begin
  Sub.new.visible
rescue NoMethodError => e
  p e.message
end
p Sub.new.send(:visible)
p Base.new.visible

# Visibility is inherited from the ancestor's entry, not re-read off the `def`.
p Base.private_method_defined?(:hidden)
p Sub.private_method_defined?(:hidden)
p Reopened.public_method_defined?(:hidden)
p Reopened.new.hidden

# Nothing was moved: the ancestor keeps its own answer.
p Base.private_instance_methods(false).sort
p Base.instance_methods(false).sort
