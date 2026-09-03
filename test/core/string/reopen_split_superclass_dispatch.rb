# Imported from the spinel corpus at c55d9bdb.
# A class opened BARE and reopened with a superclass is ruby's own
# `TypeError: superclass mismatch for class Sub` -- a real bare `class Sub`
# gives the class Object as its parent, and a later `< Base` conflicts with
# that. A forward SHELL (zeo's own device for a name a later file defines)
# carries no such history and the reopen establishes its parent instead.
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

  class Sub < Base                 # reopen: declares a conflicting superclass
    def extra
      "x"
    end
  end
end

class Child < M::Sub
  def hook
    true
  end
end

puts Child.new.run
__END__
#@ stderr
core/string/reopen_split_superclass_dispatch.rb:25:in '<module:M>': superclass mismatch for class Sub (TypeError)
	from core/string/reopen_split_superclass_dispatch.rb:7:in '<main>'
#@ exit 1
