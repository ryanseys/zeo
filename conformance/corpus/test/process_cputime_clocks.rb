# The CPU-time clock ids resolve to Integers. Their exact value is the
# platform's own clock id (it differs between Linux and macOS), so assert the
# type, not the value. Regression for #2666.
p((Process::CLOCK_PROCESS_CPUTIME_ID rescue $!.class).is_a?(Integer))
p((Process::CLOCK_THREAD_CPUTIME_ID rescue $!.class).is_a?(Integer))
p(Process::CLOCK_MONOTONIC.is_a?(Integer))
p(Process::CLOCK_REALTIME.is_a?(Integer))
