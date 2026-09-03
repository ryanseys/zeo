# `private_class_method` marks a class method the same way `private` marks an
# instance one: the name stays callable from inside the class's own class
# methods and through `send`, but an explicit receiver raises, and the default
# reflection stops reporting it.
#
# Enforcement is compile-time, like the instance half -- so a call zeo resolves
# statically reads the visibility the class BODY declared. A `private_class_method`
# run later, against an already-compiled class, still changes what the
# reflection reports but cannot retract a call site already bound (zeo's
# documented Path-1 posture, which the instance half shares).
def err
  yield
rescue NoMethodError => e
  [:nome, e.message]
end

class A
  def self.pub = helper
  def self.helper = 21
  private_class_method :helper
end
p A.pub
p err { A.helper }
p A.respond_to?(:helper)
p A.respond_to?(:helper, true)
p A.singleton_methods
p A.send(:helper)
p err { A.public_send(:helper) }
p A.method(:helper).call

# The singleton class's instance methods ARE the class's class methods, and
# report the same visibility.
sc = A.singleton_class
p sc.instance_methods(false).sort
p sc.private_instance_methods(false)
p sc.private_method_defined?(:helper)
p sc.public_method_defined?(:helper)
p sc.public_method_defined?(:pub)

# The `def` form, on a module.
module M
  def self.pub2 = h2
  private_class_method def self.h2 = 7
end
p M.pub2
p err { M.h2 }

# `public_class_method` puts it back.
class B
  def self.helper = 1
  private_class_method :helper
  public_class_method :helper
end
p B.helper
p B.respond_to?(:helper)

# A subclass inherits the mark, and can promote it back.
class C < A; end
p err { C.helper }
p C.pub
class Promoted < A
  public_class_method :helper
end
p Promoted.helper

# An explicit `self` receiver is allowed, exactly as for a private instance
# method.
class S
  def self.entry = self.secret
  def self.secret = :ok
  private_class_method :secret
end
p S.entry

# `new` is a class method like any other.
class D
  private_class_method :new
  def self.build = new
end
p err { D.new }
p D.build.class

# Several names at once. Ruby resolves each right here, so the `def`s have to
# come first -- `private_class_method` on an undefined name is a NameError.
class Multi
  def self.one = 1
  def self.two = 2
  def self.three = 3
  private_class_method :one, :two
end
p err { Multi.one }
p err { Multi.two }
p Multi.three
p Multi.singleton_methods.sort
__END__
21
[:nome, "private method 'helper' called for class A"]
false
true
[:pub]
21
[:nome, "private method 'helper' called for class A"]
21
[:pub]
[:helper]
true
false
true
7
[:nome, "private method 'h2' called for module M"]
1
true
[:nome, "private method 'helper' called for class C"]
21
21
:ok
[:nome, "private method 'new' called for class D"]
D
[:nome, "private method 'one' called for class Multi"]
[:nome, "private method 'two' called for class Multi"]
3
[:three]
