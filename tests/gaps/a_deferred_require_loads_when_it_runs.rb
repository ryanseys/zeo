# A `require` written inside a METHOD body defines its feature's constants
# EAGERLY, before the program's first statement, where CRuby defines them when
# the method runs.
#
# HALF FIXED 2026-08-24. The size half is closed; the eager-registration half
# is not, and it is the same cause as
# `tests/gaps/an_autoloaded_units_body_runs.rb`.
#
# WHAT WAS WRONG. `Hir::deferred_requires` documents the rule -- CRuby loads
# such a file when the method runs, so the call stays a runtime
# `Kernel#require` -- and the loader records the file in `single_unit_demand`
# so it IS compiled in. But `Hir::uses_runtime_eval` matched a call NAME, so
# every surviving `require` read as "this program can compile Ruby at run
# time" and linked the whole embedded compiler. This program cost
# **24,064,600 bytes**; it now costs **9,969,400**, against `puts 1`'s
# 9,781,736.
#
# `narrow_runtime_eval` now drops a plain `require` whose LITERAL feature
# names a unit this compile emitted. That is exact rather than optimistic:
# `dynamic_require` asks `features::load_feature` BEFORE the on-disk tier that
# needs a compiler, so a registered unit never reaches it. The two shapes that
# still carry the compiler both do so correctly -- a feature that resolved to
# no unit (`require "no_such_feature"`), and a COMPUTED target, both measured
# at 23,981,832 bytes.
#
# `require_relative` and `load` stay conservative on purpose. A
# `require_relative` resolves against the calling file's directory, which is a
# run-time fact; `load` re-executes and asks DISK first, so the unit is not
# the row it takes.
#
# WHAT IS LEFT is line 1 below. A compiled-in unit's classes are registered in
# the dispatch tables from startup -- `FeatureUnit`'s own doc says so -- so
# `defined?(PrettyPrint)` answers `"constant"` before anything required it.
# The unit's BODY does wait for the require, which is why lines 2-4 agree.
# Fixing it means registering a unit's classes when the unit runs, not at
# boot, and that is the same work `an_autoloaded_units_body_runs.rb` needs.

def lazy
  require "prettyprint"
end

p defined?(PrettyPrint)
p lazy
p defined?(PrettyPrint)
p lazy
