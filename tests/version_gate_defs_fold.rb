# Version-gate guards wrapping whole definitions fold at compile time against
# zeo's fixed target version, exactly as CRuby decides them at load time.
# `RUBY_VERSION` is a build-time String, so `RUBY_VERSION <op> "x"` folds by the
# same lexicographic String comparison CRuby evaluates natively -- letting this
# fixture's oracle and zeo agree without any runtime stub. (The `Gem::Version`
# segment path is covered by `guard_fold`'s unit tests, which can't share a
# fixture with a real-rubygems oracle.)

class Widget
  # "4.0.5" < "3.0" is false -> the compat module + prepend are dropped (a
  # conditional `prepend` zeo cannot express under static MRO).
  if RUBY_VERSION < "3.0"
    module LegacyPatch
      def name
        "patched"
      end
    end
    prepend LegacyPatch
  end

  # unless ("4.0.5" >= "3.0" == true) -> also dropped.
  unless RUBY_VERSION >= "3.0"
    class Nested
      def obsolete; end
    end
  end

  def name
    "plain widget"
  end
end

# A true top-level gate keeps its class (analyze registers the taken branch).
if RUBY_VERSION >= "3.0"
  class Alive
    def status
      "alive"
    end
  end
end

# The dropped `prepend LegacyPatch` never takes effect, so `name` is the plain
# body method, not "patched" -- the observable proof the false-guarded compat
# definition folded away.
puts Widget.new.name
puts Alive.new.status
