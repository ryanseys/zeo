# A receiverless call resolves against the enclosing definition's self. A
# top-level `def` is a private method on Object, so it sits at the BOTTOM of
# every ancestry -- last, not first.
#
# Several analysis passes asked for the free functions first, so a same-named
# top-level def took the call. The one that mattered was narrow_object_arrays:
# with the call attributed to the wrong scope, the real callee's parameter was
# never joined to the caller's slot, and the two were narrowed apart -- an
# sp_PolyArray argument passed to an sp_PtrArray parameter, and the C build
# stopped (#4130; #4106 was the same rule at the parameter-binding site).
#
# The top-level methods below are never called and never passed anywhere.
Scenario = Struct.new(:key)

module Blocks
  def self.run
    scenarios = []
    heading(scenarios, "a")
    heading(scenarios, "b")
    scenarios
  end

  def self.heading(scenarios, said)
    scenarios.push(Scenario.new(said))
  end
end

def heading(a) = "top-level #{a}"

p Blocks.run.length
p Blocks.run.map { |s| s.key }

# The same collision on the INSTANCE side, where the enclosing self is an
# object rather than a module.
class Sheet
  def fill
    rows = []
    row(rows, "a")
    row(rows, "b")
    rows
  end

  def row(rows, said)
    rows.push(Scenario.new(said))
  end
end

def row(x) = "top-level row"

p Sheet.new.fill.length
p Sheet.new.fill.map { |s| s.key }

# And the top-level method still answers when it is the one actually named.
p heading("x")
p row(1)
