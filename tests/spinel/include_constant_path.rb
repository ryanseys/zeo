# `include` with a ConstantPathNode argument
# (`include A::B`, not just bare `include B`).
#
# Root cause: `collect_class_with_prefix`'s two include handlers
# (the reopen branch and the fresh-class branch) gated module
# extraction on `@nd_type[arg] == "ConstantReadNode"` only. A
# qualified path like `Foo::Bar::Baz` parses as a
# ConstantPathNode and silently fell through, so the included
# module's methods never landed on the class's method table and
# the analyzer warned `cannot resolve call to '<m>' on obj_<C>`
# at every call site.
#
# Fix: both sites also accept ConstantPathNode and flatten the
# path via `const_ref_flat_name` (`A::B` -> `A_B`) before passing
# through the existing prefix-aware resolver. The resolver
# already tries `<prefix>_<name>` first and falls back to the
# flat name, so cross-namespace and nested-class includes both
# stay correct.

module A
  module B
    def foo
      "foo-from-AB"
    end

    def greet(name)
      "hello, " + name + "!"
    end
  end
end

class Host
  include A::B
end

puts Host.new.foo
puts Host.new.greet("world")

# Three-deep path: `X::Y::Z` -> `X_Y_Z`.
module X
  module Y
    module Z
      def deep
        "deep-from-XYZ"
      end
    end
  end
end

class DeepHost
  include X::Y::Z
end

puts DeepHost.new.deep

# Including via a path also from a class nested under a module.
# The lexical-prefix probe misses (`Nested_A_B` isn't registered),
# so the resolver falls back to the flat name `A_B` and finds the
# top-level module.
module Nested
  class Sub
    include A::B

    def via_nested
      foo + " (via nested)"
    end
  end
end

puts Nested::Sub.new.via_nested

# Absolute-path include (`::Top`). Inside `module Outer` the lexical-
# prefix probe would otherwise pick `Outer_Helper` because such a
# module is registered; an absolute path must bypass that and
# resolve to the top-level `Helper`. const_ref_is_relative returns 0
# for ConstantPathNode whose receiver chain bottoms out at another
# ConstantPathNode (no ConstantReadNode root) -- that's the AST
# shape for `::Helper`.
module Helper
  def from_top
    "top-helper"
  end
end

module Outer
  module Helper
    def from_top
      "outer-helper"
    end
  end

  class Inside
    include ::Helper
  end
end

puts Outer::Inside.new.from_top

