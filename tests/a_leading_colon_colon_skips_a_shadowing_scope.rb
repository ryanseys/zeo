# `::Widget` means the TOP-LEVEL `Widget`, whatever the enclosing scopes call
# their own. zeo lowered the anchor to a read scoped to `Object`, and when that
# found nothing it re-resolved the BARE name -- through the lexical chain,
# which is precisely what the `::` prefix says not to do. Inside a scope that
# shadows the name, the anchored reference answered the shadow.
#
# Silently: the shadow is a real class, so nothing raised and nothing warned.
# `NS::Array#to_a` calling `::Array.new` reached `NS::Array.new` and got an
# ArgumentError from a constructor it never meant to call -- and a shadow with
# a compatible constructor would simply have returned the wrong object.
class Widget
  def self.who = :toplevel
end
module NS
  class Widget
    def self.who = :nested
    def anchored = ::Widget.who
    def bare = Widget.who
  end
end
p NS::Widget.new.anchored
p NS::Widget.new.bare

# The case that found it: a class shadowing a BUILTIN name, which is what any
# gem defining its own `Array`/`Hash`/`Set` does.
module Collections
  class Array
    def build = ::Array.new(2) { |i| i * 3 }
    def anchored = ::Array
    def bare = Array
  end
  class Hash
    def anchored = ::Hash
  end
end
p Collections::Array.new.build
p Collections::Array.new.anchored
p Collections::Array.new.bare
p Collections::Hash.new.anchored

# A value constant takes the same anchor.
VERSION = "top"
module Versioned
  VERSION = "nested"
  class Reader
    def anchored = ::VERSION
    def bare = VERSION
  end
end
p Versioned::Reader.new.anchored
p Versioned::Reader.new.bare

# A module shadowing its own enclosing module's name -- the anchor reaches past
# every level, not just one.
module Deep
  module Deep
    def self.who = :inner
  end
  def self.who = :outer
  class C
    def anchored = ::Deep.who
    def bare = Deep.who
  end
end
p Deep::C.new.anchored
p Deep::C.new.bare

# An arbitrary `Scope::Name` must NOT gain the same fallback: `M::V` is M's own
# constant, never the unrelated top-level one.
module M
  V = :m_owns_it
end
V = :top_level
p M::V
p ::V
