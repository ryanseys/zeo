# Ruby evaluates a superclass expression BEFORE it binds the class's own name,
# so the name being defined is never a candidate for its own superclass -- the
# lexical search walks straight past it and continues outward. Rails writes
# exactly this, literally: `class SchemaDumper < SchemaDumper` inside
# `ActiveRecord::ConnectionAdapters`, meaning `ActiveRecord::SchemaDumper`.
#
# Resolving it to the inner name instead makes a class its own parent, and a
# superclass chain that points at itself is an infinite walk: every unguarded
# `parent` loop in the compiler runs forever on it.

module App
  class Dumper
    def kind = :outer
  end

  module Adapters
    class Dumper < Dumper
      def extra = :extra
    end
  end
end

p App::Adapters::Dumper.superclass
p App::Adapters::Dumper.new.kind
p App::Adapters::Dumper.new.extra
p App::Dumper.superclass

# ... and the adapter subclasses that then inherit THROUGH it -- the rest of
# the shape, where a chain that pointed at itself would have looped.
module App
  module Adapters
    class MySQL < Adapters::Dumper
      def kind = [:mysql, super]
    end
  end
end

p App::Adapters::MySQL.new.kind
p App::Adapters::MySQL.ancestors.take(3)

# The same shape ACROSS FILES, which is how rails has it: the abstract file
# writes `class Dumper < Dumper` and the adapters inherit through it. Split
# across a require graph, the name the superclass clause must skip is one the
# other files have already referenced.
require_relative "a_class_is_not_its_own_superclass/base"
require_relative "a_class_is_not_its_own_superclass/abstract"
require_relative "a_class_is_not_its_own_superclass/adapter"

p Store::Adapters::Dumper.superclass
p Store::Adapters::Dumper.new.extra
p Store::Adapters::MySQL.new.kind
p Store::Adapters::MySQL.ancestors.take(3)

# A deeper name still wins when it is a REAL definition rather than the one
# being written: nothing here is skipped.
module App
  class Real
    def who = :top
  end

  module Adapters
    class Real
      def who = :nested
    end

    class UsesNested < Real
    end
  end
end

p App::Adapters::UsesNested.new.who
p App::Adapters::UsesNested.superclass
__END__
App::Dumper
:outer
:extra
Object
[:mysql, :outer]
[App::Adapters::MySQL, App::Adapters::Dumper, App::Dumper]
Store::Dumper
:extra
[:mysql, :base]
[Store::Adapters::MySQL, Store::Adapters::Dumper, Store::Dumper]
:nested
App::Adapters::Real
