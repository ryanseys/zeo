# GAP -- imported from the spinel corpus at fa06b601.
#
# `Process.clock_gettime` accepts a clock as a SYMBOL name in CRuby
# (`:CLOCK_MONOTONIC`) as well as a constant or a raw id. zeo takes only the
# integer, so the symbol raises `TypeError: no implicit conversion of Symbol
# into Integer`.
#
# The symbol spelling is the portable one -- a constant a platform does not
# define is a NameError, while the symbol form lets a caller rescue -- so
# this is the shape a library actually writes.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# `Process.clock_gettime` takes a clock as a SYMBOL name as well as a constant
# or a raw id. The symbol went into an Integer slot and raised (#4044).
p Process.clock_gettime(:CLOCK_MONOTONIC).class
p Process.clock_gettime(Process::CLOCK_MONOTONIC).class
p Process.clock_gettime(:CLOCK_REALTIME).class
p Process.clock_gettime(:CLOCK_MONOTONIC, :nanosecond).class
p Process.clock_gettime(:CLOCK_MONOTONIC, :millisecond).class
p Process.clock_getres(:CLOCK_MONOTONIC).class
p Process.clock_gettime(:CLOCK_REALTIME) > 0
p Process.clock_gettime(:CLOCK_PROCESS_CPUTIME_ID) >= 0
