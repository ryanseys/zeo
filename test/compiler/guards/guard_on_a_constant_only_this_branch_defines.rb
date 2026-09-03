# `defined?`/`const_defined?` ask about a MOMENT: has this name been bound yet?
# When the only definition of the name in the whole program is the branch the
# guard itself decides, the answer at the guard is "not yet" -- so the `unless`
# spelling TAKES its branch, and the `if` spelling skips one that would only
# have reopened a class something outside the program was expected to supply.

class Decimal
end

unless Object.const_defined?("Money")
  class Money < Decimal
  end
end
p Money.superclass

module PG
end

# Nothing else defines PG::CancelConnection, so the reopening below is dead --
# exactly as it is in a build carrying no C extension to reopen.
if defined?(PG::CancelConnection)
  class PG::CancelConnection
    def cancel = :cancelled
  end
end
p defined?(PG::CancelConnection)

# A guard the rest of the program DOES answer stays a real guard: `Decimal` is
# defined above, so this branch is skipped and the class keeps its own name.
unless Object.const_defined?("Decimal")
  class Decimal
    def self.name = "shadowed"
  end
end
p Decimal.name

# A literal's class is fixed at compile time, and so is its name -- msgpack
# picks between an Integer and a Fixnum reopening this way, and `Fixnum` no
# longer exists, so the untaken branch must never be looked at.
if 1.class.name == "Integer"
  class Reader
    def kind = :int
  end
else
  class Reader < Fixnum
    def kind = :fixnum
  end
end
p Reader.new.kind
__END__
Decimal
nil
"Decimal"
:int
