# Ruby resolves a bare constant by searching every entry of `Module.nesting`
# outward, then the cref's ancestors, then Object. zeo's emitted lookup jumped
# straight from the innermost scope to the top, so any name the compile-time
# owner map cannot place -- every constant on a class built at RUNTIME -- was
# searched for in two scopes where Ruby searches three or more.
#
# jmespath hit it: `class Parser` in `module JMESPath` reads
# `Token::BINDING_POWER`, and `Token` is a `Struct.new` subclass, so the lookup
# asked `JMESPath::Parser` and `Object`, skipped `JMESPath`, and raised.
module M
  class Reader
    def table
      Holder::TABLE[:star]
    end

    def deep
      Inner::Deeper.new.reach
    end
  end

  # Built at runtime, so no compile-time owner places `Holder`.
  Holder = Class.new
  Holder.const_set(:TABLE, { star: 42 })

  module Inner
    class Deeper
      # Two scopes out: Deeper -> Inner -> M.
      def reach
        Holder::TABLE[:star] + 1
      end
    end
  end
end

p M::Reader.new.table
p M::Reader.new.deep
p M::Inner::Deeper.new.reach

# The qualified form does NOT see the path prefix's constants lexically, and
# that distinction has to survive the wider search.
module Outer
  SECRET = :outer
  module Mid; end
end
class Outer::Mid::Leaf
  def peek
    defined?(SECRET) ? SECRET : :not_visible
  end
end
p Outer::Mid::Leaf.new.peek
