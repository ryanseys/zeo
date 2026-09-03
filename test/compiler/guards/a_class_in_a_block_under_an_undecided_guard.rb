# A `class`/`module` written inside a BLOCK registers at the top level -- the
# walk descends into the block body, the definition keeps its document
# position, and the class exists once the block has run.
#
# Under an UNDECIDED top-level guard the branch's own definitions register as
# runtime-conditional, but an interleaved statement was kept whole and never
# descended into, so a class inside a block inside the branch reached codegen
# unregistered. core_ex is the shape: a `silence_warnings do module Inflector`
# inside `unless defined? CORE_EX_LOADED`, whose condition the branch itself
# assigns.
def wrap
  yield
end

unless defined?(GUARD_LOADED) && GUARD_LOADED
  GUARD_LOADED = true

  # Registered by the guarded-branch walk, as it already was.
  class Plain
    def hi = "plain"
  end

  wrap do
    module Inflector
      class Inflections
        def hi = "inflections"
      end

      def self.hi = "inflector"
    end
  end

  wrap do
    class Plain::Nested
      def hi = "nested"
    end
  end
end

p Plain.new.hi
p Inflector.hi
p Inflector::Inflections.new.hi
p Plain::Nested.new.hi
p [Inflector.class, Inflector::Inflections.superclass, Plain::Nested.name]

# The guard really is undecided: a second pass over the same shape is a no-op,
# so nothing was defined twice.
unless defined?(GUARD_LOADED) && GUARD_LOADED
  wrap do
    module Inflector
      def self.hi = "redefined"
    end
  end
end
p Inflector.hi
__END__
"plain"
"inflector"
"inflections"
"nested"
[Module, Object, "Plain::Nested"]
"inflector"
