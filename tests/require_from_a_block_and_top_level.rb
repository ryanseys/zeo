# The SAME file required from inside a block AND at top level. The two
# requires used to take different paths and disagree about who runs the file
# body: one marked the feature loaded without running the body's statements,
# which then suppressed the other's run. The `def`s survived -- methods are
# compile-time -- but a constant assigned in the module body was lost from
# BOTH requires, not just the second. ONE require of a given file was always
# correct on its own; the defect needed both forms.
#
# Closed by splicing a nested require where it is WRITTEN rather than at the
# head of the file. Found on `gems/time/lib/time.rb`, which lost
# `Time::ZoneOffset`; guarded here on `abbrev`, the smallest vendored gem with
# a module-body constant.
[1].each do
  require "abbrev"
  p Abbrev::VERSION
end

require "abbrev"
p Abbrev::VERSION
p Abbrev.abbrev(["ruby"])["rub"]
