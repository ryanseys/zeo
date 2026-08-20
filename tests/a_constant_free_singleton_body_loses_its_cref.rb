# A `def` written in a `class << self` body has the SINGLETON as its cref, so
# a `def` in a block inside it defines an instance method of that singleton --
# a class method of the enclosing class.
#
# The cref a `def` in expression position uses is the body's own
# `defining_class`, and for a CLASS-METHOD body that is `Scope::lexical_home`
# -- the singleton's surrogate -- falling back to the class only where no
# singleton was involved. Reading `defining_class` alone put the nested `def`
# on the class instead, which is the shape the name of this file records.
#
# The constant-bearing twin below is kept beside it: a `class << self` body
# holding a constant reaches the same answer by a different route, and a fix
# must not trade one for the other.
class Bar
  class << self
    def run
      [1].each { def inner = 3 }
    end
  end
end
Bar.run
p Bar.singleton_class.instance_methods(false).sort
p Bar.public_instance_methods(false).sort

# The constant-bearing twin.
class Baz
  class << self
    K = 1
    def run
      [1].each { def inner = 3 }
    end
  end
end
Baz.run
p Baz.singleton_class.instance_methods(false).sort
p Baz.public_instance_methods(false).sort
