# Process.clock_gettime validates its unit (#2727): an unknown literal symbol
# is CRuby's ArgumentError, not a silent float_second.
r = (Process.clock_gettime(Process::CLOCK_MONOTONIC, :bogus) rescue $!.class)
p(r.is_a?(Float) ? :returned_float_no_raise : r)
p Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond).is_a?(Integer)
p Process.clock_gettime(Process::CLOCK_MONOTONIC).is_a?(Float)
