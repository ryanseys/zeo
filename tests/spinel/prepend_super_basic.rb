# Issue #720 follow-up: `super` inside a prepended method now
# routes back to the original (overridden) method body via the
# synthetic shadow registered at prepend time. Before the fix the
# super call hit the unresolved-call fallback and returned "".
module M
  def hi
    "prepend-" + super
  end
end

class C
  def hi
    "base"
  end
  prepend M
end

puts C.new.hi
