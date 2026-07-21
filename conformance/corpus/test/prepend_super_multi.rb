# Issue #720 follow-up: multiple `prepend` calls stack — each
# prepend's super walks back through the previous prepend's body
# and eventually to C's own. MRO is M2 -> M1 -> C; M2's super ->
# M1; M1's super -> C.
module M1
  def hi
    "M1(" + super + ")"
  end
end

module M2
  def hi
    "M2(" + super + ")"
  end
end

class C
  def hi
    "C"
  end
  prepend M1
  prepend M2
end

puts C.new.hi
