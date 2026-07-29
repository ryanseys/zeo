# zeo does not model class-method VISIBILITY: `private_class_method` is a
# runtime no-op, so a private class method stays callable with an explicit
# receiver where ruby raises NoMethodError.
class A
  def self.pub = helper
  def self.helper = 21
  private_class_method :helper
end
p A.pub
r1 = (A.helper rescue $!.class); p r1
r2 = (A.helper rescue $!.message); p r2
p A.singleton_class.private_method_defined?(:helper)

module M
  module_function
  def pub2 = h2
  private_class_method def h2 = 7
end
p M.pub2
r3 = (M.h2 rescue $!.class); p r3

# public_class_method puts it back
class B
  def self.helper = 1
  private_class_method :helper
  public_class_method :helper
end
p B.helper
