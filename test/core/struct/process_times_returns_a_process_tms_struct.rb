# Process.times returns a Process::Tms with Float utime/stime/cutime/cstime
# (via getrusage); the class name and struct-style inspect match CRuby.

p Process::Tms
t = Process.times
p t.class
p t.utime.class
p t.stime >= 0.0
p t.cutime.class
p t.cstime.class
p t.inspect.start_with?("#<struct Process::Tms utime=")
__END__
Process::Tms
Process::Tms
Float
true
Float
Float
true
