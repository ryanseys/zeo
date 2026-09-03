# A `def` / `define_method` / `define_singleton_method` written where a VALUE
# is expected (inside a method body, a block, an argument list) compiles to a
# runtime install. Codegen used to name the ENCLOSING CLASS as the target for
# all three, which is only ruby's rule for one of them.
#
#   A plain `def` installs on the CREF's default definee -- the enclosing
#   class, `Object` at the top level. That one was right.
#
#   `def self.x` and a literal `define_method` are ordinary SENDS TO SELF, and
#   `self` is NOT the definee outside a class body. Inside an instance method
#   `self` is the INSTANCE, so `def self.x` there lands on that object's
#   singleton and `define_method` raises NoMethodError -- an instance is no
#   Module. Naming the enclosing class answered both the other way round: the
#   method appeared on the class, and `define_method` quietly succeeded.
#
# `boxed_implicit_self` is already the "self here, as a RubyValue" rule and is
# total, so the receiver is read from it rather than re-derived.

class C
  def install_singleton
    def self.only_mine = :only_mine
  end

  def install_instance_method
    define_method(:never_reached) { :never_reached }
  end
end

c = C.new
c.install_singleton
p c.only_mine
p C.respond_to?(:only_mine)
p C.new.respond_to?(:only_mine)

begin
  c.install_instance_method
rescue NoMethodError => e
  p e.class
end

# In a CLASS body, and in a `def self.x`, `self` IS the class -- these were
# always right and must stay right.
class D
  p(def self.from_class_body = :from_class_body)

  def self.install
    def self.from_class_method = :from_class_method
  end
end
D.install
p D.from_class_body
p D.from_class_method

# A plain `def` in expression position still resolves against the CREF, not
# against `self`: at the top level that is `Object`, and the `def` evaluates
# to its own name.
p(def top_level_def = :top_level_def)
p top_level_def
p Object.instance_method(:top_level_def).owner
__END__
:only_mine
false
false
NoMethodError
:from_class_body
:from_class_body
:from_class_method
:top_level_def
:top_level_def
Object
