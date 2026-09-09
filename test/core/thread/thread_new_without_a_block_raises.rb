# (spinel issue #2978)
r = (Thread.new rescue $!.class); p r
__END__
ThreadError
