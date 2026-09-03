# `Process.clock_gettime` takes a clock as a SYMBOL name.
#
# The symbol spelling is the portable one -- a constant the platform does
# not define is a `NameError` at the read, where the symbol form is a
# rescuable `Errno::EINVAL` -- so it is the shape a library actually
# writes. The name is looked up against `Process`'s own `CLOCK_*`
# constants, so the two cannot drift.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix.
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
__END__
Float
Float
Float
Integer
Integer
Float
true
true
