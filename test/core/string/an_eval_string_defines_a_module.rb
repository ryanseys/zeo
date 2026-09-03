# `eval` of a literal string that defines a `class`/`module`. At the TOP LEVEL
# zeo parses the snippet at compile time and inlines it, so the definition is
# ordinary; inside a METHOD body it falls through to the runtime eval VM,
# which had no constant-write node -- `WHO = "made"` was the whole refusal,
# not the `module` the gap header blamed.
#
# The second half is the cref: an eval'd `def self.hi` reading a bare constant
# resolves against the module it was DEFINED into. Deriving that from the
# receiver answers `Module`, which knows none of the module's constants.
def make
  eval('module Made
    WHO = "made"
    INNER = [WHO, 1].freeze
    def self.hi = "made hi #{WHO}"
    class Deep
      D = :d
    end
  end')
end

make
p [Made::WHO, Made.hi, Made.class]
p [Made::INNER, Made::Deep::D, Made.const_get(:WHO)]
p Made.constants.sort

# A reopen from a second eval adds to the same module.
def more
  eval('module Made
    SECOND = 2
  end')
end
more
p [Made::SECOND, Made.constants.sort]

# A scoped write, and the value an assignment answers.
module Ns; end
def scoped
  eval('Ns::SCOPED = 3')
end
p [scoped, Ns::SCOPED]

# ...and the top-level form, which the compile-time path already covered.
class Base
  def hi = "base"
end

eval('class Derived < Base
  def hi = "derived (#{super})"
end')
p Derived.new.hi
__END__
["made", "made hi made", Module]
[["made", 1], :d, "made"]
[:Deep, :INNER, :WHO]
[2, [:Deep, :INNER, :SECOND, :WHO]]
[3, 3]
"derived (base)"
