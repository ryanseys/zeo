# A class opened bare first (just to hold a nested class) and reopened with a
# superclass: the parent link must come from the reopen that declares it, so a
# subclass override dispatches against the right ancestor chain.
# (matz/spinel#1477. NB: MRI rejects this exact split as a superclass mismatch;
# it arises in spinel from wholesale-inlined libraries, so the expected output
# is checked in rather than regenerated from CRuby.)
module M
  class Base
    def run
      hook ? "blocked" : "ok"
    end
    def hook
      false
    end
  end

  class Sub                        # bare opening: holds a nested class
    class Nested
      def z
        1
      end
    end
  end

  class Sub < Base                 # reopen: declares the superclass + a method
    def extra
      "x"
    end
  end
end

class Child < M::Sub               # subclass overrides the hook
  def hook
    true
  end
end

puts Child.new.run                 # blocked (override dispatched)
puts M::Sub.new.run                # ok (base hook, inherited)
puts M::Sub.new.extra              # x (reopen method present)
puts M::Sub::Nested.new.z          # 1 (bare-opening nested class intact)
