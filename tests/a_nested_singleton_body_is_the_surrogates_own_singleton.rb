# `class << self` inside `class << self` opens the SINGLETON's own singleton --
# the same construct one level deeper, so it takes the same route one level
# deeper. Its body becomes a reopen of the `#<Class:self>` surrogate, where a
# `def` retagged as a class method of the surrogate IS an instance method of
# `Foo.singleton_class.singleton_class`, which is where ruby puts it.
#
# lita spells the corpus's whole cluster this way -- 565 rows behind one file --
# and its method is `private`, called by the DSL beside it.
class Authorization
  def self.plain = :plain

  class << self
    class << self
      private

      def define_deprecated(name)
        define_method(name) { "deprecated:#{name}" }
      end
    end

    define_deprecated :add_user
    define_deprecated :remove_user
  end
end

p Authorization.add_user
p Authorization.remove_user
p Authorization.plain

# The nested `def` lands two levels out, and stays private there. A call with
# NO receiver is the only way ruby reaches it -- which is what the singleton
# body beside it writes.
p Authorization.singleton_class.singleton_class.private_method_defined?(:define_deprecated)
p Authorization.singleton_class.singleton_class.public_method_defined?(:define_deprecated)
p Authorization.singleton_class.instance_methods(false).sort

# A receiver the SOURCE wrote gets ruby's answer, not the rebound form's --
# only the rebound `self.singleton_class` zeo synthesizes for a receiverless
# call is exempt. (The message names the surrogate a module where ruby names
# it a class -- see tests/gaps/a_singleton_class_surrogate_is_a_module.rb.)
begin
  Authorization.singleton_class.define_deprecated(:nope)
rescue NoMethodError => e
  p e.class
end

# A PUBLIC nested def is an ordinary class method of the singleton class, and
# reflection agrees about its owner.
class Reflected
  class << self
    class << self
      def maker = :maker
    end
  end
end
p Reflected.singleton_class.maker
p Reflected.singleton_class.method(:maker).owner == Reflected.singleton_class.singleton_class

# A constant written in the nested body is read lexically by the `def` beside
# it, and `RESULT` -- a constant whose value the statement order decides --
# sees the method the nested body just defined. (WHERE the constant lives is
# the one-level-deeper form of zeo's existing singleton-constant divergence;
# see tests/gaps/a_nested_singleton_bodys_constant_is_hoisted.rb.)
class WithConst
  class << self
    class << self
      SUFFIX = "!"
      def shout(s) = s + SUFFIX
    end
    RESULT = shout("hi")
  end
end
p WithConst.singleton_class::RESULT

# An ordinary `class` written inside a singleton body ENDS the run: its own
# `class << self` is its own singleton, not the surrogate's.
class Outer
  class << self
    class Inner
      class << self
        def who = :inners_class_method
      end
    end
    # `Inner` by bare name -- ruby scopes it to the singleton class, zeo to
    # the enclosing one, and both resolve it from here. (`Outer::Inner` is
    # where the two part company; see docs/COMPATIBILITY.md.)
    def outer_side = Inner.who
  end
end
p Outer.outer_side
p Outer.singleton_class.singleton_class.method_defined?(:who)

# Three levels deep is the same recursion again.
class Deep
  class << self
    class << self
      class << self
        def level3 = :level3
      end
    end
  end
end
p Deep.singleton_class.singleton_class.level3
