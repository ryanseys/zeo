# `is_a?` / `kind_of?` / `instance_of?` with a qualified-constant
# argument (ConstantPathNode like `Mod::Klass`). The dynamic-arg
# branch in compile_introspection_expr previously excluded only the
# bare ConstantReadNode shape; a qualified constant fell through and
# emitted compile_expr(arg) as a bare C identifier with no declaration,
# producing "use of undeclared identifier" at C compile time. The
# is_a? branch also didn't route kind_of? or instance_of? through
# the same static collapse, so those names silently fell through to
# warn_unresolved_call. Fix: resolve the constant name via the path
# walker, and dispatch the whole introspection family uniformly.
#
# Coverage:
# - Same-class match (is_a?/kind_of?/instance_of? all true)
# - Unrelated-sibling mismatch (all false)
# - Ancestor case (is_a?/kind_of? true; instance_of? false, exact only)
# - Deeper constant path (Mod::Sub::Deep — three-level path walk)
#
# Out of scope for this PR:
# - Class-as-local (`k = Mod::Klass; obj.is_a?(k)`) emits the const
#   name as a bare C identifier — a separate ConstantPathNode-to-local
#   assignment bug, distinct from the introspection-arg fix here.
# - Built-in ancestor (`obj.is_a?(Object)`) returns false in spinel
#   today; spinel's class registry doesn't materialize Object as an
#   ancestor of user classes. Separate gap.

module Mod
  class Base
    def base_name; "base"; end
  end
  class Klass < Base
    def initialize(v); @v = v; end
    def get; @v; end
  end
  class Other
    def initialize(v); @v = v; end
  end
  module Sub
    class Deep
      def initialize(v); @v = v; end
      def get; @v; end
    end
  end
end

k = Mod::Klass.new(7)
puts k.is_a?(Mod::Klass)
puts k.kind_of?(Mod::Klass)
puts k.instance_of?(Mod::Klass)
puts k.is_a?(Mod::Other)
puts k.instance_of?(Mod::Other)
puts k.is_a?(Mod::Base)
puts k.kind_of?(Mod::Base)
puts k.instance_of?(Mod::Base)
puts k.get

d = Mod::Sub::Deep.new(42)
puts d.is_a?(Mod::Sub::Deep)
puts d.kind_of?(Mod::Sub::Deep)
puts d.instance_of?(Mod::Sub::Deep)
puts d.get
