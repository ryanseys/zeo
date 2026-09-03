r = (Thread.new rescue $!.class); p r
__END__
ThreadError
