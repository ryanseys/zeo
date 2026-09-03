# `Process` identity and clocks -- asserted as FACTS about the values, never
# the values themselves (a pid isn't reproducible).

p Process.pid.is_a?(Integer)
p Process.pid > 0
p Process.clock_gettime(Process::CLOCK_MONOTONIC).is_a?(Float)
p Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond).is_a?(Integer)
p Process.clock_gettime(Process::CLOCK_MONOTONIC, :float_second).is_a?(Float)
a = Process.clock_gettime(Process::CLOCK_MONOTONIC)
b = Process.clock_gettime(Process::CLOCK_MONOTONIC)
p b >= a
__END__
true
true
true
true
true
true
