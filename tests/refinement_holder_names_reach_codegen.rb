# A refinement's holder module is named `#refinement:<Target>` -- deliberately
# unspellable as a Ruby constant, so it claims no name inside the refining
# module, and it is the same name CRuby prints for `M.refinements`.
#
# `#` and `:` are not Rust identifier characters, so every generated ident
# built from a class NAME has to sanitize. `class_ident`'s nested arm did not,
# and `Ident::new` panics rather than raising: four gems in the corpus (caps,
# kgl, kuport, lambit) died with `"__c405_#refinement:String" is not a valid
# Ident` instead of compiling.
#
# The holder only reaches that ident when it has CLASS methods to emit, which
# is why plain refinements never tripped it. `module_function` inside `refine`
# is one way -- it makes each `def` a module function of the holder, i.e. a
# singleton method -- and it is exactly what kgl 0.0.9 ships.
module Sizes
  refine Integer do
    module_function

    def kb = self * 1024
  end
end

# `def self.x` inside `refine` is the other way the holder gets a class method,
# and the same ident is built for it.
module Helpers
  refine Float do
    def self.unit = "px"

    def rounded = round(2)
  end
end

# `refine ::Hash` anchors at top level, which renders as `::Hash` and flattens
# to a LEADING DOT: `#refinement:.Hash`. lambit 0.0.2 died on this one.
module Anchored
  refine ::Hash do
    def second_value = values[1]
  end
end

# A qualified target flattens `::` to `.` so the name reads as one leaf.
module Outer
  class Inner
    def initialize(n) = @n = n
    def n = @n
  end
end

module Qualified
  refine Outer::Inner do
    def doubled = n * 2
  end
end

# Two refinements of DIFFERENT targets in one module: their holders must stay
# distinct idents, not collapse into each other.
module Pair
  refine String do
    def shout = upcase + "!"
  end

  refine Symbol do
    def shout = to_s.upcase + "!"
  end
end

using Sizes
using Helpers
using Anchored
using Qualified
using Pair

# `module_function` made `kb` a PRIVATE instance method of the refinement, so
# the refined call needs an explicit send -- the point here is that the holder
# reached codegen at all, not the visibility.
p 2.send(:kb)
p 3.14159.rounded
p({ a: 1, b: 2 }.second_value)
p Outer::Inner.new(21).doubled
p "hi".shout
p :hi.shout

# The NAME is Ruby-visible and must not have changed -- only the Rust ident it
# is mangled into.
p Pair.refinements.map { |r| r.to_s }.sort
p Anchored.refinements.map { |r| r.to_s }
p Sizes.refinements.size
p Pair.constants
