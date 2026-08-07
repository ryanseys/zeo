# Ruby preloads rubygems, so a gem reaches for `Gem::Version` without ever
# requiring it -- and rubygems' `<=>` COERCES a plain string operand, making the
# comparison version-segment-wise rather than lexicographic. The two answer
# differently often enough to matter: `"10.0"` is greater than `"9.0"` as a
# version and smaller as a string.
#
# unparser gates its two `Builder` definitions on one of these, one per branch
# with a DIFFERENT superclass. A compiler that can't decide the guard registers
# both arms and reports a superclass mismatch for a program ruby runs without
# complaint, so deciding it is what makes the gem compile at all.

module Unparsed
  if Gem::Version.new(RUBY_VERSION) <= "3.4"
    class Builder < String
      def which = :legacy
    end
  else
    class Builder < Array
      def which = :modern
    end
  end
end

p Unparsed::Builder.superclass
p Unparsed::Builder.new.which

# The same gate the other way round, so BOTH branches are exercised: no ruby
# this compiles under is past 99.
module Backport
  if Gem::Version.new(RUBY_VERSION) <= "99.0"
    class Shim < Array
      def which = :shimmed
    end
  else
    class Shim < String
      def which = :native
    end
  end
end

p Backport::Shim.superclass
p Backport::Shim.new.which

# Version semantics, proved: a string comparison would order these the other
# way, and would pick the other branch.
if Gem::Version.new("10.0") > "9.0"
  class TenIsNewer
    def ordering = :by_version
  end
else
  class TenIsNewer
    def ordering = :by_string
  end
end

p TenIsNewer.new.ordering

# `String#<=>` hands an operand it can't compare back to that operand's own
# `<=>` and negates the answer, so a version on the RIGHT still orders by
# version.
module Reversed
  if "9.0" < Gem::Version.new("10.0")
    class Order < Array
      def which = :by_version
    end
  else
    class Order < String
      def which = :by_string
    end
  end
end

p Reversed::Order.superclass
p Reversed::Order.new.which

# Two versions, no coercion needed.
module Both
  if Gem::Version.new(RUBY_VERSION) >= Gem::Version.new("3.0")
    class Pick < Array
      def which = :at_least_three
    end
  else
    class Pick < String
      def which = :older
    end
  end
end

p Both::Pick.superclass
p Both::Pick.new.which
