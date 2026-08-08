# A singleton class IS a Class in ruby. zeo's `#<Class:self>` surrogate is
# registered as a MODULE (`is_module: true`), so every question about its own
# identity answers the other way. Only the identity questions diverge -- the
# methods and constants it holds are unaffected, which is why the surrogate
# has carried this since it was first minted for constants alone.
#
# Making it a class means giving it a superclass, and ruby's answer there is
# `#<Class:Object>` -- another singleton class -- so the fix is a chain, not a
# flag.
#
# Only a body that MINTS the surrogate is affected. Without one, the runtime
# mints an ordinary singleton class and every answer below is ruby's.
class K
  class << self
    NEEDS_A_SURROGATE = true
    def a = :a
  end
end
p K.singleton_class.class
p K.singleton_class.is_a?(Class)
p K.singleton_class.instance_of?(Module)

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
