# Followup to #404. Built-in exception classes (StandardError,
# RuntimeError, ...) and nested user classes (M::Inner) used as
# values must lower to sp_Class compound literals, the same way
# flat user-class constants already did. Pre-fix the emit was a
# bare C identifier (`StandardError`, `M_Inner`) that gcc rejected
# with "use of undeclared identifier". Issue #637.

# Shape A: built-in exception class as value.
def takes_class(c)
  puts c.name
end

takes_class(StandardError)
takes_class(RuntimeError)
takes_class(ArgumentError)

# Class object via a local — round-trip through a value slot.
c = StandardError
puts c.name

# Shape B: nested user class as value. Verify the value compiles
# and survives a round-trip through a local; the displayed
# `c.name` between spinel and MRI may differ (spinel uses `_` as
# the internal separator), but `c == Inner-class` confirms the
# value emit and equality semantics hold.
module M
  class Inner
  end
end

def make
  M::Inner
end

c2 = make
puts c2 == M::Inner ? "round-trip ok" : "round-trip BROKEN"
