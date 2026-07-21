r = Process.clock_getres(Process::CLOCK_MONOTONIC)
puts r.class
puts r > 0
puts Process.clock_getres(Process::CLOCK_REALTIME, :nanosecond).class
puts Process.clock_getres(Process::CLOCK_MONOTONIC, :float_second).class
