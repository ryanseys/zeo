# A singleton class IS a Class in ruby. zeo's compile-time `#<Class:self>`
# surrogate -- the class a `class << self` body's constants, `def`s and
# residual statements register under -- was filed as a MODULE, because holding
# constants was all the registry first needed of it. Every question about its
# own identity then answered the other way, including in user-visible text.
#
# Registering it as a class is the whole fix: `superclass: None` puts Object
# behind it, and nothing in the MRO depended on the module-ness (`class <<
# self` items dispatch through the singleton tables, not the surrogate's
# ancestry).
class K
  class << self
    NEEDS_A_SURROGATE = true
    def a = :a
  end
end
p K.singleton_class.class
p K.singleton_class.is_a?(Class)
p K.singleton_class.instance_of?(Module)
p K.singleton_class.is_a?(Module)
p K.a
p K.singleton_class.const_get(:NEEDS_A_SURROGATE)
p K.const_defined?(:NEEDS_A_SURROGATE)

# It reaches user-visible text: a private class method's NoMethodError names
# the receiver's kind.
class Priv
  class << self
    class << self
      private
      def hidden = :hidden
    end
  end
end
begin
  Priv.singleton_class.hidden
rescue NoMethodError => e
  p e.message
end

# A body with no constant at all mints the surrogate too, and answers alike.
class Plain
  class << self
    def b = :b
  end
end
p Plain.singleton_class.class
p Plain.b

# An ORDINARY module is untouched by the change.
module Mod; end
p Mod.class
p Mod.is_a?(Class)
p Mod.singleton_class.class
__END__
Class
true
false
true
:a
true
false
"private method 'hidden' called for class #<Class:Priv>"
Class
:b
Module
false
Class
