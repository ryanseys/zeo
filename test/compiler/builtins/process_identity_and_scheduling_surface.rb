# Process ids/scheduling against libc, plus the PRIO_* selector constants.

p [Process.uid.class, Process.gid.class, Process.euid.class, Process.egid.class]
p [Process.getpgrp.class, Process.getsid.class]
p Process.clock_getres(Process::CLOCK_MONOTONIC).class
p Process.clock_getres(Process::CLOCK_REALTIME, :nanosecond).class
p [Process::PRIO_PROCESS, Process::PRIO_PGRP, Process::PRIO_USER]
p Process.groups.all? { |g| g.is_a?(Integer) }
p Process.getpriority(Process::PRIO_PROCESS, 0).class
__END__
[Integer, Integer, Integer, Integer]
[Integer, Integer]
Float
Integer
[0, 1, 2]
true
Integer
