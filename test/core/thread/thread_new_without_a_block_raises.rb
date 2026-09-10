# `Thread.new` with no block has nothing to run, so it raises rather than
# starting a thread that returns at once.
r = (Thread.new rescue $!.class); p r
__END__
ThreadError
