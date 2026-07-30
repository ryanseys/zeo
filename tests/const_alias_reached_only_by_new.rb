# A constant aliasing a class, defined in a LEXICAL PARENT and reached only
# through `.new` from a class nested inside it. The ownership collector saw a
# bare `Fast` read but not `Fast.new`, so the name got no ownership entry and
# codegen's runtime fallback looked it up on the referencing class -- a
# `NameError` for a constant that is plainly in scope. prism's serializer is
# written exactly this way (`FastStringIO = ::StringIO`, then
# `FastStringIO.new` inside its `Loader`).
module M
  module S
    Fast = ::String

    class Loader
      def build = Fast.new("hi")
    end
  end
end
p M::S::Loader.new.build

# A bare read of the same name elsewhere in the class used to be what made it
# work -- it must keep working, and must not be what makes it work.
module N
  module T
    Fast = ::String

    class Loader
      def named = Fast
      def build = Fast.new("ok")
    end
  end
end
p N::T::Loader.new.named
p N::T::Loader.new.build

# The same shape one level deeper, and through a qualified path.
module U
  Wrapped = ::Array

  module V
    class Maker
      def build = Wrapped.new(2, 0)
      def qualified = ::U::Wrapped.new(1, 9)
    end
  end
end
p U::V::Maker.new.build
p U::V::Maker.new.qualified
