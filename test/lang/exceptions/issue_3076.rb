p((Signal.signame(nil) rescue $!.class))
p((Signal.signame(true) rescue $!.class))
p((Signal.signame("INT") rescue $!.class))
p((Signal.signame(:INT) rescue $!.class))
p(Signal.signame(9))
__END__
TypeError
TypeError
TypeError
TypeError
"KILL"
