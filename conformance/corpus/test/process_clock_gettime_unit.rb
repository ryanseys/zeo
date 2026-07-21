# clock_gettime honors the clock id and unit. Values are machine time, so
# assert structure/type (platform-neutral), not exact numbers.
p(Process.clock_gettime(Process::CLOCK_REALTIME) > 1_000_000_000)
p(Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond).class)
p(Process.clock_gettime(Process::CLOCK_MONOTONIC, :second).class)
p(Process.clock_gettime(Process::CLOCK_MONOTONIC, :float_second).class)
p(Process::CLOCK_MONOTONIC.is_a?(Integer))
