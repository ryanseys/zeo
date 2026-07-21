# Issue #720 follow-up: prepended super walks through C's own body
# down to the parent class. MRO is M -> C -> P; M's super -> C's
# body; C's body's super -> P's body.
module M
  def hi
    "M(" + super + ")"
  end
end

class P
  def hi
    "p"
  end
end

class C < P
  def hi
    "c(" + super + ")"
  end
  prepend M
end

puts C.new.hi
