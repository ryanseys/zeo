# A `def` written in a `class << self` body has the SINGLETON as its cref, so
# a `def` in a block inside it defines an instance method of that singleton --
# a class method of the enclosing class. zeo puts it on the class instead.
#
# WHY. The cref a `def` in expression position uses is `Scope::lexical_home`,
# which is the singleton's surrogate class -- and analyze only tags a def with
# one when the `class << self` body also holds a CONSTANT, because that is the
# only thing that mints a surrogate `ClassDef` (lower/defs.rs, one reopen per
# constant). A constant-free singleton body has no surrogate to name, so its
# defs fall back to the enclosing class.
#
# ADD A CONSTANT AND IT IS RIGHT: the same program with `K = 1` in the
# singleton body answers exactly as ruby does, which is what says the cref
# machinery is sound and only the tagging is short.
#
# SHAPE OF A FIX: mint the surrogate for every `class << self` body rather than
# per constant, and tag every def beside it. That also changes `Module.nesting`
# and bare-constant resolution for every such def, so it wants its own measured
# pass rather than riding along here.
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

# The constant-bearing twin, which already agrees -- kept here so a fix cannot
# trade one for the other.
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
