# `class Map < Collection::MapImplementation` -- concurrent-ruby's shape, where
# the superclass is named through a QUALIFIED constant that holds a class.
#
# `const_write_values` keys the scope-less writes by their LEAF, so
# `MapImplementation = ...` written inside `module Collection` is filed under
# that leaf alone and the path never matched it. The subclass was judged to
# have a compile-time superclass, went down the static path, and was DROPPED
# when nothing there could resolve the name -- `Concurrent::Map` simply did not
# exist, and i18n could not build its cache on top of it.
module D
  module Coll
    class Backend
      def kind = :backend
    end
    IMPL = Backend
  end

  class ViaQualified < Coll::IMPL
    def extra = :extra
  end
  class ViaFullyQualified < D::Coll::IMPL; end
end

# An UNQUALIFIED alias always worked, and a qualified real class always took
# the static path. Both stay here so a fix to one cannot quietly move the other.
module E
  class Backend
    def kind = :backend
  end
  IMPL = Backend
  class ViaBare < IMPL; end
  class ViaRealClass < E::Backend; end
end

p D::ViaQualified.new.kind
p D::ViaQualified.new.extra
p D::ViaQualified.superclass
p D::ViaFullyQualified.new.kind
p E::ViaBare.new.kind
p E::ViaRealClass.new.kind
p D::ViaQualified.ancestors.include?(D::Coll::Backend)
