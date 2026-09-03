# One file demanded twice under two different names: an `autoload` naming it
# on the load path, and a guarded `require_relative` naming it by path. rack
# writes both, and the second never runs.
#
# The two demands reach the loader in DIFFERENT rounds of its unit sweep --
# the `require_relative` while `prog.rb` lowers, the `autoload` only once
# `pkg.rb` is itself spliced as a unit -- so a unit registered under
# whichever name arrived first dropped the other. When the one dropped was
# the autoload's, the read that should have run the unit found no unit under
# that name and did nothing: `Pkg::Target` still resolved (a unit's classes
# are in the dispatch tables from startup) and `Pkg::Target.read` was still
# callable, so the only symptom was its body's constant missing.

$LOAD_PATH.unshift(File.expand_path("an_autoload_and_a_guarded_require_of_one_file_share_its_unit/lib", __dir__))
require_relative "an_autoload_and_a_guarded_require_of_one_file_share_its_unit/prog"
__END__
pkg ran
before read
target ran
:split
