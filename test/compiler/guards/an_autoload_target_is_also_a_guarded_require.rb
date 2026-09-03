# One file, demanded twice under two different names. rack writes both:
#
#   lib/rack.rb          autoload :MediaType, "rack/media_type"
#   test/spec_media.rb   separate_testing { require_relative "../lib/rack/media_type" }
#
# The second never runs (that method does not yield), so the autoload is the
# only thing that can load the file -- and it has to still work.
#
# A unit registered under whichever name the loader saw first dropped the
# other, and when the one dropped was the autoload's, the read that should
# have run the unit found no unit under that name -- silently, because a
# unit's classes are in the dispatch tables from startup. `Outer::Target`
# resolved, `Outer::Target.read` was callable, and only its body's constant
# was missing:
#
#   NameError: uninitialized constant #<Class:Outer::Target>::SPLIT
#
# This file pins the ORDER OF EVENTS against ruby. The regression test for
# the drop itself is `an_autoload_and_a_guarded_require_of_one_file_share_
# its_unit` in `tests/e2e/gems_require.rs`: reproducing it needs the autoload
# to name a LOAD-PATH feature, so a name that resolves to nothing has no file
# to fall back to, and a golden has no `-I` of its own.

def self.never_yields
  :no_yield
end

# The file holding the `autoload` is itself behind a guard, which is what puts
# its demand in a later round. HOME is set in any test environment.
require_relative "an_autoload_target_is_also_a_guarded_require/lib" if ENV["HOME"]

# The rack shape: a guarded require of the same file that never runs.
never_yields do
  require_relative "an_autoload_target_is_also_a_guarded_require/target"
end

# Nothing has read the constant, so nothing has loaded the target.
p Outer.autoload?(:Target) != nil
p Object.const_defined?(:MISSING_MARKER)

# The read is what runs it -- and its body's constants are there.
p Outer::Target.read
p Outer.autoload?(:Target) != nil
__END__
lib.rb ran
true
false
target.rb ran
:split
false
